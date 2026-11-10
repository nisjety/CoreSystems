//! Provider configuration and model catalog loaded from environment variables.

use anyhow::{Context, Result};

use mp_contracts::model_entitlement::Mode;

use crate::metrics;
use crate::provider::zdr::{ZdrAttestation, ZdrResolution};
use crate::provider::{Bearer, Residency};

/// Top-level configuration for inference-core.
// The flags are independent deny-by-default residency/retention opt-ins, not a
// state machine: each guards a different boundary and any combination is valid.
// Same rationale as `ProviderCapabilities`' capability bits.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    /// Ordered list of provider names to try (e.g. [`anthropic`, `openai`]).
    pub provider_order: Vec<String>,

    /// Anthropic API key (from `ANTHROPIC_API_KEY`).
    pub anthropic_api_key: Option<String>,

    /// OpenAI-compatible API base URL (from `OPENAI_API_BASE`).
    pub openai_api_base: Option<String>,

    /// `OpenAI` API key (from `OPENAI_API_KEY`).
    pub openai_api_key: Option<String>,

    /// Internal Integration Core URL for ChatGPT subscription-backed Codex
    /// execution (from `CODEX_SUBSCRIPTION_INTEGRATION_CORE_URL`). The broker
    /// keeps managed ChatGPT auth; this service gets only an opaque connection
    /// id on each request.
    pub codex_subscription_integration_core_url: Option<String>,

    /// Service credential accepted by Integration Core's internal broker route
    /// (from `CODEX_SUBSCRIPTION_INTERNAL_API_KEY`). It authenticates this
    /// service, not the end user's ChatGPT subscription.
    pub codex_subscription_internal_api_key: Option<String>,

    /// Exact Codex models enabled for subscription-backed execution (from
    /// `CODEX_SUBSCRIPTION_MODELS`). A non-empty catalog is required before the
    /// provider registers, so it cannot capture arbitrary OpenAI model traffic.
    pub codex_subscription_models: Vec<String>,

    /// Whether a call that names a customer-gateway model connection may be served (from
    /// `INFERENCE_BYO_GATEWAY_ENABLED`, the literal `true` only). The gateway is reached through the
    /// Integration Core URL above, which the subscription route shares. Off, such a call is refused, and no
    /// platform provider ever serves it.
    pub byo_gateway_enabled: bool,

    /// The service credential Integration Core's customer-gateway broker accepts from Model Plane (from
    /// `CUSTOMER_GATEWAY_INTERNAL_API_KEY`, falling back to `CODEX_SUBSCRIPTION_INTERNAL_API_KEY` as the broker
    /// itself falls back). It authenticates this service, not the customer's endpoint.
    pub customer_gateway_internal_api_key: Option<String>,

    /// Azure `OpenAI` endpoint (from `AZURE_OPENAI_ENDPOINT`).
    pub azure_openai_endpoint: Option<String>,

    /// Azure `OpenAI` API key (from `AZURE_OPENAI_API_KEY`).
    pub azure_openai_api_key: Option<String>,

    /// Azure `OpenAI` API version (from `AZURE_OPENAI_API_VERSION`).
    pub azure_openai_api_version: String,

    /// `OpenAI` chat model catalog (from `OPENAI_CHAT_MODELS`).
    pub openai_chat_models: Vec<String>,

    /// `OpenAI` embedding model catalog (from `OPENAI_EMBEDDING_MODELS`).
    pub openai_embedding_models: Vec<String>,

    /// Azure `OpenAI` chat deployment catalog (from `AZURE_OPENAI_CHAT_DEPLOYMENTS`
    /// or legacy `AZURE_OPENAI_DEPLOYMENT`).
    pub azure_openai_chat_deployments: Vec<String>,

    /// Azure `OpenAI` embedding deployment catalog (from
    /// `AZURE_OPENAI_EMBEDDING_DEPLOYMENTS` or legacy
    /// `AZURE_OPENAI_EMBEDDING_DEPLOYMENT`).
    pub azure_openai_embedding_deployments: Vec<String>,

    /// Azure AI Foundry Anthropic (Claude) resource base endpoint, e.g.
    /// `https://<resource>.services.ai.azure.com` (from `AZURE_ANTHROPIC_ENDPOINT`).
    /// When set with a key, Claude requests route here instead of
    /// `api.anthropic.com` — the fix for the direct API's exhausted credit.
    pub azure_anthropic_endpoint: Option<String>,

    /// Azure AI Foundry Anthropic API key (from `AZURE_ANTHROPIC_API_KEY`).
    pub azure_anthropic_api_key: Option<String>,

    /// Deployed Claude deployment-name catalog on the Azure Foundry resource
    /// (from `AZURE_ANTHROPIC_DEPLOYMENTS`). Used for `list_models`.
    pub azure_anthropic_deployments: Vec<String>,

    /// Azure AI Foundry unified inference endpoint hosting Cohere's `MaaS` chat
    /// deployment, e.g. `https://core-ai-rg.services.ai.azure.com/models`
    /// (from `AZURE_COHERE_ENDPOINT`). Every non-self-hosted model must come
    /// from an Azure AI Foundry deployment — this is Command A Plus's, the
    /// live successor to the retired "Command R+" model name.
    pub azure_cohere_endpoint: Option<String>,

    /// API key for the Azure Cohere resource (from `AZURE_COHERE_API_KEY`).
    /// Kept separate from `azure_openai_api_key` rather than falling back to
    /// it, matching the Azure Anthropic precedent: same-resource reuse is
    /// common but not guaranteed, so this must be configured explicitly.
    pub azure_cohere_api_key: Option<String>,

    /// API version for the unified AI Model Inference route (from
    /// `AZURE_COHERE_API_VERSION`). Defaults to `2024-05-01-preview`, the
    /// version live-verified against this account's Cohere deployments.
    pub azure_cohere_api_version: String,

    /// Deployment name of the Command A Plus chat model on the Azure Cohere
    /// resource (from `AZURE_COHERE_DEPLOYMENT`). Chat-only: this provider
    /// declares no embedding catalog.
    pub azure_cohere_deployment: String,

    /// Explicit residency region of the Azure Cohere resource (from
    /// `AZURE_COHERE_REGION`). Same rationale as
    /// [`Self::azure_anthropic_region`] — a separate resource, so it does not
    /// inherit `azure_openai_region`'s geography.
    pub azure_cohere_region: Option<String>,

    /// Maximum retries per provider before falling back.
    pub max_retries_per_provider: u32,

    /// Prompt cache TTL in seconds.
    pub cache_ttl_secs: u64,

    /// Master switch for the Verevon intent layer (from `VEREVON_INTENT_ENABLED`,
    /// default on). When off, `verevon-*` model ids fall through to the legacy
    /// per-provider default resolution.
    pub verevon_intent_enabled: bool,

    /// cost-core base URL for the intent layer's budget check (from
    /// `COST_CORE_URL`). `None` disables the budget gate (posture is always
    /// `Unknown`, so routing uses the healthy ladder).
    pub cost_core_url: Option<String>,

    /// Monthly USD budget cap used as the denominator for the budget posture
    /// (from `VEREVON_INTENT_BUDGET_USD`, default 50.0). Seeds the bootstrap
    /// `RoutingPolicy::budget_cap_usd`; a session-core policy overrides it.
    pub verevon_intent_budget_usd: f64,

    /// session-core base URL for the runtime `RoutingPolicy` store (from
    /// `SESSION_CORE_URL` or the compose `SESSION_CORE_ADDR`). `None` disables
    /// the runtime policy store — the chain runs on `RoutingPolicy::default`.
    pub session_core_url: Option<String>,

    /// How often inference-core re-polls session-core for the live policy, in
    /// seconds (from `ROUTER_POLICY_REFRESH_SECS`, default 60).
    pub router_policy_refresh_secs: u64,

    /// Explicit residency region of the configured Azure embedding deployment
    /// (from `AZURE_OPENAI_REGION`, e.g. `swedencentral`). Azure `OpenAI`
    /// endpoint hosts don't carry the region, so this is the authoritative
    /// signal the startup residency gate classifies. Empty/unset means
    /// "unspecified" — the endpoint-substring heuristic is then the only signal.
    pub azure_openai_region: Option<String>,

    /// Explicit residency region of the Azure AI Foundry Claude resource (from
    /// `AZURE_ANTHROPIC_REGION`).
    ///
    /// Separate from [`Self::azure_openai_region`] because these are separate
    /// Azure resources: the checked-in configuration points them at
    /// `core-ai-rg.cognitiveservices.azure.com` and
    /// `cloude-ai-resource.services.ai.azure.com`. Inheriting one's geography for
    /// the other would assert an EU boundary for a resource nothing has verified —
    /// and Foundry Claude's regional availability is narrower than Azure
    /// `OpenAI`'s, so they are quite likely to differ in practice.
    pub azure_anthropic_region: Option<String>,

    /// Evidence-bound operator attestation that the configured Azure `OpenAI`
    /// deployment is covered by an independently verified ZDR contract (from
    /// `AZURE_OPENAI_ZDR_*`). Region alone is not evidence of provider retention
    /// behavior, and neither is a boolean — see [`ZdrAttestation`].
    pub azure_openai_zdr: Option<ZdrAttestation>,

    /// The same attestation for the Azure AI Foundry Claude resource (from
    /// `AZURE_ANTHROPIC_ZDR_*`). Separate from the `OpenAI` one because they are
    /// separate Azure resources under separate retention approvals; before this
    /// existed, no Claude route could serve a ZDR request at all.
    pub azure_anthropic_zdr: Option<ZdrAttestation>,

    /// Declared Azure `OpenAI` deployment type (from
    /// `AZURE_OPENAI_DEPLOYMENT_TYPE`, e.g. `DataZoneStandard`, `Standard`,
    /// `GlobalStandard`). Azure endpoint hosts carry neither the region nor the
    /// deployment type, so this is the only signal that distinguishes an
    /// EU-boundary deployment from a `Global` one that may process the request
    /// in any region worldwide — which silently voids the EU residency claim.
    pub azure_openai_deployment_type: Option<String>,

    /// Deny-by-default override for the global-deployment gate (from
    /// `MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT`, default `false`). When `false`, an
    /// explicitly `Global`/`Worldwide` Azure deployment type fails the service
    /// loud at startup rather than serving traffic under an EU claim it cannot
    /// honor.
    pub allow_global_deployment: bool,

    /// Deny-by-default override for the provider residency registration gate
    /// (from `MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY`, default `false`). When `false`,
    /// a provider declaring no residency commitment refuses to register. The
    /// direct vendor APIs (`api.openai.com`, `api.anthropic.com`) declare
    /// `Global`, so a development box pointed at them needs this opt-in.
    pub allow_global_residency_providers: bool,

    /// Deny-by-default override for the EU embedding residency gate (from
    /// `MODEL_PLANE_ALLOW_NON_EU_EMBEDDING`, default `false`). When `false`, a
    /// non-EU embedding region/endpoint fails the service loud at startup and
    /// rejects requests; setting it `true` is an explicit operator opt-in to
    /// egress embeddings outside the EU residency boundary.
    pub allow_non_eu_embedding: bool,
}

impl InferenceConfig {
    /// Build configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if `INFERENCE_PROVIDER_ORDER` is unset.
    // Linear env-var-to-field reads plus the residency/ZDR wiring; reads
    // top-to-bottom and isn't worth fragmenting across helpers.
    #[allow(clippy::too_many_lines)]
    pub fn from_env() -> Result<Self> {
        let provider_order = std::env::var("INFERENCE_PROVIDER_ORDER")
            .unwrap_or_else(|_| "anthropic,openai".to_owned())
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        let max_retries: u32 = std::env::var("INFERENCE_MAX_RETRIES")
            .unwrap_or_else(|_| "3".to_owned())
            .parse()
            .context("INFERENCE_MAX_RETRIES must be a valid u32")?;

        let cache_ttl: u64 = std::env::var("INFERENCE_CACHE_TTL_SECS")
            .unwrap_or_else(|_| "300".to_owned())
            .parse()
            .context("INFERENCE_CACHE_TTL_SECS must be a valid u64")?;

        // Verevon intent layer — on unless explicitly disabled with a falsey value.
        let verevon_intent_enabled = std::env::var("VEREVON_INTENT_ENABLED")
            .map(|v| {
                !matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "off" | "no"
                )
            })
            .unwrap_or(true);

        let cost_core_url = std::env::var("COST_CORE_URL")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());

        let verevon_intent_budget_usd: f64 = std::env::var("VEREVON_INTENT_BUDGET_USD")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(50.0);

        // Accept either env name: deployments wire `SESSION_CORE_ADDR` (compose)
        // while `SESSION_CORE_URL` is the documented primary — same precedence
        // as execution-core.
        let session_core_url = std::env::var("SESSION_CORE_URL")
            .or_else(|_| std::env::var("SESSION_CORE_ADDR"))
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());

        let router_policy_refresh_secs: u64 = std::env::var("ROUTER_POLICY_REFRESH_SECS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(60);

        let azure_openai_region = trimmed_env("AZURE_OPENAI_REGION");
        let azure_anthropic_region = trimmed_env("AZURE_ANTHROPIC_REGION");
        let azure_cohere_region = trimmed_env("AZURE_COHERE_REGION");

        // One `today` for the whole boot so every provider surface evaluates its
        // in-force window against the same date.
        let today = chrono::Utc::now().date_naive();
        let azure_openai_zdr = resolve_zdr(
            "azure_openai",
            "AZURE_OPENAI",
            "AZURE_OPENAI_ZDR_CONFIRMED",
            today,
        )
        .context("Azure OpenAI ZDR attestation")?;
        let azure_anthropic_zdr = resolve_zdr(
            "azure_anthropic",
            "AZURE_ANTHROPIC",
            "AZURE_ANTHROPIC_ZDR_CONFIRMED",
            today,
        )
        .context("Azure Anthropic (Foundry Claude) ZDR attestation")?;

        let azure_openai_deployment_type = trimmed_env("AZURE_OPENAI_DEPLOYMENT_TYPE");

        // Deny-by-default, same shape as the EU embedding gate below.
        let allow_global_deployment = truthy_env("MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT");
        let allow_global_residency_providers = truthy_env("MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY");

        // Deny-by-default: only an explicit truthy opt-in disables the EU
        // embedding residency gate. Mirrors the speech.rs MODEL_PLANE_ALLOW_NON_EU_TTS
        // shape but the embedding gate REJECTS rather than warn-and-fallback.
        let allow_non_eu_embedding = truthy_env("MODEL_PLANE_ALLOW_NON_EU_EMBEDDING");

        Ok(Self {
            provider_order,
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            openai_api_base: std::env::var("OPENAI_API_BASE").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            codex_subscription_integration_core_url: trimmed_env(
                "CODEX_SUBSCRIPTION_INTEGRATION_CORE_URL",
            ),
            codex_subscription_internal_api_key: trimmed_env("CODEX_SUBSCRIPTION_INTERNAL_API_KEY"),
            codex_subscription_models: csv_env("CODEX_SUBSCRIPTION_MODELS", &[]),
            byo_gateway_enabled: byo_gateway_switch_on(
                std::env::var("INFERENCE_BYO_GATEWAY_ENABLED").ok().as_deref(),
            ),
            customer_gateway_internal_api_key: trimmed_env("CUSTOMER_GATEWAY_INTERNAL_API_KEY")
                .or_else(|| trimmed_env("CODEX_SUBSCRIPTION_INTERNAL_API_KEY")),
            azure_openai_endpoint: std::env::var("AZURE_OPENAI_ENDPOINT").ok(),
            azure_openai_api_key: std::env::var("AZURE_OPENAI_API_KEY").ok(),
            azure_openai_api_version: std::env::var("AZURE_OPENAI_API_VERSION")
                .unwrap_or_else(|_| "2025-01-01-preview".to_owned()),
            openai_chat_models: csv_env("OPENAI_CHAT_MODELS", &["gpt-4o-mini", "gpt-5-mini"]),
            openai_embedding_models: csv_env(
                "OPENAI_EMBEDDING_MODELS",
                &["text-embedding-3-small", "text-embedding-3-large"],
            ),
            azure_openai_chat_deployments: csv_env_with_legacy(
                "AZURE_OPENAI_CHAT_DEPLOYMENTS",
                "AZURE_OPENAI_DEPLOYMENT",
            ),
            azure_openai_embedding_deployments: csv_env_with_legacy(
                "AZURE_OPENAI_EMBEDDING_DEPLOYMENTS",
                "AZURE_OPENAI_EMBEDDING_DEPLOYMENT",
            ),
            azure_anthropic_endpoint: std::env::var("AZURE_ANTHROPIC_ENDPOINT").ok(),
            azure_anthropic_api_key: std::env::var("AZURE_ANTHROPIC_API_KEY").ok(),
            azure_anthropic_deployments: csv_env(
                "AZURE_ANTHROPIC_DEPLOYMENTS",
                &[
                    "claude-haiku-4-5",
                    "claude-sonnet-4-5",
                    "claude-sonnet-4-6",
                    "claude-opus-4-8",
                ],
            ),
            azure_cohere_endpoint: std::env::var("AZURE_COHERE_ENDPOINT").ok(),
            azure_cohere_api_key: std::env::var("AZURE_COHERE_API_KEY").ok(),
            azure_cohere_api_version: std::env::var("AZURE_COHERE_API_VERSION")
                .unwrap_or_else(|_| "2024-05-01-preview".to_owned()),
            azure_cohere_deployment: std::env::var("AZURE_COHERE_DEPLOYMENT")
                .unwrap_or_else(|_| "cohere-command-a-plus".to_owned()),
            max_retries_per_provider: max_retries,
            cache_ttl_secs: cache_ttl,
            verevon_intent_enabled,
            cost_core_url,
            verevon_intent_budget_usd,
            session_core_url,
            router_policy_refresh_secs,
            azure_openai_region,
            azure_anthropic_region,
            azure_cohere_region,
            azure_openai_zdr,
            azure_anthropic_zdr,
            azure_openai_deployment_type,
            allow_global_deployment,
            allow_global_residency_providers,
            allow_non_eu_embedding,
        })
    }
}

/// Resolve one provider surface's evidence-bound ZDR attestation.
///
/// ZDR is evidence-bound as of the provider-strategy Phase 0 work. The legacy
/// `*_ZDR_CONFIRMED` boolean is still read, but only so an operator who asserted
/// ZDR without supplying the attestation fails boot: silently dropping the claim
/// would leave them believing ZDR is on while requests fail later with
/// `ZdrUnavailable`, far from the change that caused it.
///
/// An approval outside its in-force window is the one thing that does not fail
/// boot (DEC-33 D6): the surface boots without ZDR, so ZDR turns refuse with the
/// typed `zdr_unavailable`, an ERROR is logged and the review-by gauge keeps
/// reporting the date. Failing boot there made a forgotten review date a whole
/// inference outage, though only ZDR traffic depends on the approval.
fn resolve_zdr(
    surface: &'static str,
    prefix: &str,
    legacy_var: &'static str,
    today: chrono::NaiveDate,
) -> Result<Option<ZdrAttestation>> {
    let legacy_confirmed = std::env::var(legacy_var)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true"))
        .unwrap_or(false);
    let resolution = ZdrAttestation::resolve_lenient(prefix, legacy_var, legacy_confirmed, today)?;
    if let Some(attestation) = resolution.attestation() {
        metrics::record_zdr_attestation_review_by(surface, attestation.review_by());
    }
    if let ZdrResolution::NotInForce(not_in_force) = &resolution {
        tracing::error!(
            surface,
            review_by = %not_in_force.attestation().review_by(),
            "ZDR attestation is not in force; this surface will refuse ZDR requests until it is \
             re-verified and its date and digest are updated: {}",
            not_in_force.reason()
        );
    }
    Ok(resolution.into_in_force())
}

/// Whether a declared Azure deployment type processes requests outside a single
/// geography.
///
/// `Global`/`GlobalStandard`/`GlobalBatch` and anything spelled `worldwide` route
/// to whichever Azure region has capacity, so data at rest may sit in the
/// European geography while inference happens anywhere. `DataZone*` and plain
/// regional `Standard` deployments stay inside their declared boundary.
#[must_use]
pub fn deployment_type_is_global(declared: &str) -> bool {
    let token = declared.trim().to_ascii_lowercase().replace(['-', '_'], "");
    token.starts_with("global") || token.contains("worldwide")
}

/// The org-core service id inference-core presents when it pulls an organisation's
/// model entitlement (ADR-0005). org-core's registry names this principal and
/// grants it only `org:model-entitlement:read:any`.
pub const ORG_CORE_SERVICE_ID: &str = "inference-core";

/// How inference-core reads and applies the organisation's model entitlement.
///
/// Kept apart from [`InferenceConfig`] because nothing on the provider side
/// reads it: only the entitlement gate does, and it stays a unit that can be
/// built without the provider settings.
#[derive(Debug, Clone)]
pub struct EntitlementConfig {
    /// `MODEL_ENTITLEMENT_MODE`: `off` (the default), `shadow` or `enforce`. A
    /// value that is none of them is `enforce`: see [`Mode::parse`].
    pub mode: Mode,
    /// org-core's base URL (from `ORG_CORE_URL`). Without it a `shadow` or
    /// `enforce` mode has nothing to ask, so every answer is unavailable.
    pub org_core_url: Option<String>,
    /// The credential org-core accepts for [`ORG_CORE_SERVICE_ID`] (from
    /// `INFERENCE_ORG_CORE_SERVICE_TOKEN`). Redacted from `Debug`.
    pub service_token: Bearer,
}

impl EntitlementConfig {
    /// Entitlement off: no settings, no calls.
    #[must_use]
    pub fn off() -> Self {
        Self {
            mode: Mode::Off,
            org_core_url: None,
            service_token: Bearer::default(),
        }
    }

    /// Read the settings from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            mode: Mode::parse(std::env::var("MODEL_ENTITLEMENT_MODE").ok().as_deref()),
            org_core_url: trimmed_env("ORG_CORE_URL"),
            service_token: Bearer::new(
                trimmed_env("INFERENCE_ORG_CORE_SERVICE_TOKEN").unwrap_or_default(),
            ),
        }
    }
}

/// An environment variable trimmed to `None` when unset or blank.
fn trimmed_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// A deny-by-default boolean flag: only an explicit `1`/`true` enables it.
fn truthy_env(name: &str) -> bool {
    std::env::var(name)
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true"))
        .unwrap_or(false)
}

fn csv_env(name: &str, default: &[&str]) -> Vec<String> {
    std::env::var(name)
        .ok()
        .map(|value| parse_csv(&value))
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| default.iter().map(|value| (*value).to_owned()).collect())
}

fn csv_env_with_legacy(name: &str, legacy: &str) -> Vec<String> {
    std::env::var(name)
        .or_else(|_| std::env::var(legacy))
        .ok()
        .map(|value| parse_csv(&value))
        .unwrap_or_default()
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Vertex AI Gemini settings. They sit apart from [`InferenceConfig`] so every existing
/// configuration keeps building unchanged, and the provider stays off unless
/// `VERTEX_GEMINI_ENABLED` is the literal `true`.
#[derive(Debug, Clone, Default)]
pub struct VertexGeminiConfig {
    /// `VERTEX_GEMINI_ENABLED`. Only the literal `true` enables the provider.
    pub enabled: bool,
    /// `VERTEX_GEMINI_PROJECT`, the Google Cloud project the models are called in.
    pub project: String,
    /// `VERTEX_GEMINI_LOCATION`. Registration needs a location on the EU allowlist.
    pub location: String,
    /// `VERTEX_GEMINI_MODELS`, comma separated: the only models this endpoint serves.
    pub models: Vec<String>,
    /// `VERTEX_GEMINI_CREDENTIALS_FILE`, a mounted service-account key. Only the path is
    /// held here; the key is read at registration and never printed.
    pub credentials_file: String,
}

impl VertexGeminiConfig {
    /// Read the settings from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            enabled: vertex_switch_on(std::env::var("VERTEX_GEMINI_ENABLED").ok().as_deref()),
            project: trimmed_env("VERTEX_GEMINI_PROJECT").unwrap_or_default(),
            location: trimmed_env("VERTEX_GEMINI_LOCATION").unwrap_or_default(),
            models: trimmed_env("VERTEX_GEMINI_MODELS")
                .map(|value| parse_csv(&value))
                .unwrap_or_default(),
            credentials_file: trimmed_env("VERTEX_GEMINI_CREDENTIALS_FILE").unwrap_or_default(),
        }
    }
}

/// The strict reading of `INFERENCE_BYO_GATEWAY_ENABLED`: only the literal `true` enables it, as it does
/// for the Vertex switch. Anything else, including `1`, `TRUE` and an empty value, leaves it off.
fn byo_gateway_switch_on(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// The strict reading of `VERTEX_GEMINI_ENABLED`: only the literal `true` enables it.
/// Anything else, including `1`, `TRUE` and an empty value, leaves the provider off.
fn vertex_switch_on(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// The Vertex locations that keep processing in the EU: the EU multi-region, and the
/// European regions the Vertex locations page lists. The page does not confirm which of
/// them serve each model, so the enablement gate checks that per model.
const VERTEX_EU_LOCATIONS: &[&str] = &[
    "eu",
    "europe-central2",
    "europe-north1",
    "europe-southwest1",
    "europe-west1",
    "europe-west2",
    "europe-west3",
    "europe-west4",
    "europe-west6",
    "europe-west8",
    "europe-west9",
];

/// Whether `location` is one of [`VERTEX_EU_LOCATIONS`].
#[must_use]
pub(crate) fn is_vertex_eu_location(location: &str) -> bool {
    VERTEX_EU_LOCATIONS.contains(&location.trim().to_ascii_lowercase().as_str())
}

/// The residency a Vertex location declares. Only an allowlisted location is EU; anything
/// else is `Global`, so the residency floors skip it.
#[must_use]
pub(crate) fn vertex_residency(location: &str) -> Residency {
    if is_vertex_eu_location(location) {
        Residency::Eu
    } else {
        Residency::Global
    }
}

#[cfg(test)]
mod tests {
    use super::{
        deployment_type_is_global, is_vertex_eu_location, vertex_residency, vertex_switch_on,
    };
    use crate::provider::Residency;

    #[test]
    fn vertex_location_allowlist() {
        for location in ["eu", "europe-west4", "EU", " europe-north1 "] {
            assert!(is_vertex_eu_location(location), "{location:?} is EU");
        }
        for location in [
            "us-central1",
            "global",
            "asia-northeast1",
            "europe-west10",
            "",
        ] {
            assert!(!is_vertex_eu_location(location), "{location:?} is not EU");
        }
    }

    #[test]
    fn global_location_classifies_global() {
        assert_eq!(vertex_residency("us-central1"), Residency::Global);
        assert_eq!(vertex_residency("global"), Residency::Global);
        assert_eq!(vertex_residency("europe-west4"), Residency::Eu);
    }

    #[test]
    fn vertex_switch_is_on_only_for_the_literal_true() {
        assert!(vertex_switch_on(Some("true")));
        assert!(vertex_switch_on(Some(" true ")));
        for raw in [
            None,
            Some(""),
            Some("1"),
            Some("TRUE"),
            Some("yes"),
            Some("on"),
        ] {
            assert!(!vertex_switch_on(raw), "{raw:?} must not enable the switch");
        }
    }

    /// Every spelling Azure uses for a deployment that may process a request in
    /// any region worldwide. Getting this wrong silently voids the EU residency
    /// claim, so the classifier is tested against the real value set rather than
    /// one representative string.
    #[test]
    fn global_deployment_types_are_classified_as_global() {
        for declared in [
            "Global",
            "GlobalStandard",
            "GlobalProvisionedManaged",
            "GlobalBatch",
            "global-standard",
            "global_standard",
            "  GLOBALSTANDARD  ",
            "Worldwide",
        ] {
            assert!(
                deployment_type_is_global(declared),
                "{declared:?} must be classified as a global deployment"
            );
        }
    }

    /// EU-boundary deployment types must not trip the gate, or the fix becomes a
    /// boot failure for correctly configured deployments.
    #[test]
    fn boundary_respecting_deployment_types_are_not_global() {
        for declared in [
            "DataZoneStandard",
            "DataZoneProvisionedManaged",
            "DataZoneBatch",
            "Standard",
            "ProvisionedManaged",
            "datazone-standard",
            "",
        ] {
            assert!(
                !deployment_type_is_global(declared),
                "{declared:?} must not be classified as a global deployment"
            );
        }
    }
}
