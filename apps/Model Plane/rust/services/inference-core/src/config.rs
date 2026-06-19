//! Provider configuration and model catalog loaded from environment variables.

use anyhow::{Context, Result};

/// Top-level configuration for inference-core.
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

    /// Maximum retries per provider before falling back.
    pub max_retries_per_provider: u32,

    /// Prompt cache TTL in seconds.
    pub cache_ttl_secs: u64,

    /// Master switch for the Velion intent layer (from `VELION_INTENT_ENABLED`,
    /// default on). When off, `velion-*` model ids fall through to the legacy
    /// per-provider default resolution.
    pub velion_intent_enabled: bool,

    /// cost-core base URL for the intent layer's budget check (from
    /// `COST_CORE_URL`). `None` disables the budget gate (posture is always
    /// `Unknown`, so routing uses the healthy ladder).
    pub cost_core_url: Option<String>,

    /// Monthly USD budget cap used as the denominator for the budget posture
    /// (from `VELION_INTENT_BUDGET_USD`, default 50.0). Seeds the bootstrap
    /// `RoutingPolicy::budget_cap_usd`; a session-core policy overrides it.
    pub velion_intent_budget_usd: f64,

    /// session-core base URL for the runtime `RoutingPolicy` store (from
    /// `SESSION_CORE_URL` or the compose `SESSION_CORE_ADDR`). `None` disables
    /// the runtime policy store — the chain runs on `RoutingPolicy::default`.
    pub session_core_url: Option<String>,

    /// How often inference-core re-polls session-core for the live policy, in
    /// seconds (from `ROUTER_POLICY_REFRESH_SECS`, default 60).
    pub router_policy_refresh_secs: u64,
}

impl InferenceConfig {
    /// Build configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if `INFERENCE_PROVIDER_ORDER` is unset.
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

        // Velion intent layer — on unless explicitly disabled with a falsey value.
        let velion_intent_enabled = std::env::var("VELION_INTENT_ENABLED")
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

        let velion_intent_budget_usd: f64 = std::env::var("VELION_INTENT_BUDGET_USD")
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

        Ok(Self {
            provider_order,
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            openai_api_base: std::env::var("OPENAI_API_BASE").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
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
            max_retries_per_provider: max_retries,
            cache_ttl_secs: cache_ttl,
            velion_intent_enabled,
            cost_core_url,
            velion_intent_budget_usd,
            session_core_url,
            router_policy_refresh_secs,
        })
    }
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
