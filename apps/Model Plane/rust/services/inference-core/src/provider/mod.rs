//! Provider routing traits and implementations.

pub mod anthropic;
pub mod artifact_ref;
pub mod doc_intel;
pub mod fallback;
pub mod intent;
pub mod language;
pub mod openai;
pub mod policy_client;
pub mod realtime;
pub mod routing_policy;
pub mod speech;
pub mod translation;
pub mod video;
pub mod vision;
pub mod zdr;

#[allow(unused_imports)]
// ArtifactStore is part of the intended provider surface; not yet consumed
pub use artifact_ref::{ArtifactRef, ArtifactStore};

use tokio::sync::mpsc;

/// Shared HTTP client for outbound provider calls, with bounded timeouts.
///
/// Uses a connect timeout plus a READ (inactivity) timeout rather than a total
/// deadline: a connected-but-unresponsive upstream stops sending bytes and is
/// dropped within the read window — freeing the retry/fallback chain to advance
/// to the next provider — while a slow-but-progressing (e.g. streaming or
/// long-generation) response keeps resetting the read timer and is never
/// falsely killed. Previously every provider built `reqwest::Client::new()` with
/// no timeout, so a hung upstream blocked the request indefinitely.
///
/// Both bounds are env-tunable; falls back to a default client if the builder
/// ever fails (so a misconfig can never take the process down).
#[must_use]
pub(crate) fn provider_http_client() -> reqwest::Client {
    fn env_secs(key: &str, default: u64) -> u64 {
        std::env::var(key)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(default)
    }
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(env_secs(
            "INFERENCE_PROVIDER_CONNECT_TIMEOUT_SECS",
            10,
        )))
        .read_timeout(std::time::Duration::from_secs(env_secs(
            "INFERENCE_PROVIDER_READ_TIMEOUT_SECS",
            120,
        )))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Narrow an `f64` (e.g. a JSON confidence score or embedding value) to `f32`.
///
/// The precision loss is intentional and inherent to `f64 -> f32`; there is no
/// lossless conversion, so the cast lint is allowed on this single helper rather
/// than at every provider call site.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub(crate) fn narrow_f64(value: f64) -> f32 {
    value as f32
}

/// A bearer token that refuses to print itself.
///
/// A newtype rather than a hand-written `Debug` on the containing struct: that
/// alternative silently starts leaking the day someone adds a field and lets
/// `#[derive(Debug)]` come back. This way every struct holding a bearer — now or
/// later — is safe by construction. `auth::AuthenticatedPrincipal` already
/// redacts its own bearer by hand; this is the same discipline made reusable.
///
/// Empty is distinguished from present so "was the token forwarded at all?"
/// stays debuggable without exposing the value.
#[derive(Clone, Default)]
pub struct Bearer(String);

impl Bearer {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Bearer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(if self.0.is_empty() {
            "Bearer([empty])"
        } else {
            "Bearer([REDACTED])"
        })
    }
}

/// A unified inference request used internally across providers.
#[derive(Debug, Clone, Default)]
pub struct InferRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: i32,
    pub structured_output_schema: Option<String>,
    pub zdr: bool,
    /// chat-parity §2 function-calling: tools the model may call (empty = none).
    pub tools: Vec<ToolDefinition>,
    /// Tool selection policy: "auto" | "none" | "required" | a tool name.
    pub tool_choice: String,
    /// Tenant scope for the Verevon intent layer's budget check (from gRPC
    /// metadata `x-org-id`; empty when the caller doesn't forward it).
    pub org_id: String,
    /// Acting user for the budget check (from gRPC metadata `x-user-id`).
    pub user_id: String,
    /// The caller's own verified bearer token (the gRPC `authorization`
    /// metadata already validated by `grpc::authorize`), forwarded to
    /// cost-core so the intent layer's budget check authenticates as the
    /// caller. **Auth material for the budget check ONLY** — it MUST never be
    /// serialized into any provider request body (provider adapters build
    /// their bodies from explicit fields, never from this struct wholesale).
    /// Empty when there is no delegated caller (internal sub-calls, tests):
    /// the budget check is then skipped and posture stays `Unknown`.
    ///
    /// Typed as [`Bearer`] so the struct's `#[derive(Debug)]` cannot print it.
    pub caller_bearer: Bearer,
}

/// A function the model may call (chat-parity §2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema (as a JSON string) for the tool's parameters.
    pub parameters_json: String,
}

/// A model-requested tool invocation (chat-parity §2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

/// A single chat message.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// Unified inference response.
#[derive(Debug, Clone, Default)]
pub struct InferResponse {
    pub request_id: String,
    pub content: String,
    pub model_used: String,
    pub stop_reason: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    /// chat-parity §2: tool calls the model requested (empty for a plain answer).
    pub tool_calls: Vec<ToolCall>,
}

/// A single streaming chunk.
#[derive(Debug, Clone)]
pub struct InferChunk {
    pub request_id: String,
    pub delta: String,
    pub done: bool,
    pub model_used: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
}

/// A unified embedding request used internally across providers.
#[derive(Debug, Clone, Default)]
pub struct EmbedRequest {
    pub request_id: String,
    pub provider_hint: String,
    pub text: String,
    pub model: String,
    /// Zero Data Retention. Threaded from the gRPC request so a future EU/ZDR
    /// provider can honor it; inference-core's provider is Azure today, so
    /// residency enforcement here is a Phase-4 concern — this just carries the
    /// signal end-to-end (it does NOT by itself satisfy residency).
    pub zdr: bool,
    /// Requested/required residency region for the embedding deployment (e.g.
    /// `swedencentral`, `westeurope`). The fallback chain enforces this
    /// deny-by-default: a non-EU region is rejected before any network call
    /// unless `MODEL_PLANE_ALLOW_NON_EU_EMBEDDING` is set. Empty means the
    /// caller expresses no preference and the configured EU deployment is used.
    pub region: String,
}

/// Unified embedding response.
#[derive(Debug, Clone)]
pub struct EmbedResponse {
    pub request_id: String,
    pub vector: Vec<f32>,
    pub model_used: String,
    pub provider_used: String,
}

/// Provider model/deployment metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub modality: String,
    pub streaming: bool,
    /// chat-parity §2 per-model feature families (e.g. "reasoning", "tools",
    /// "vision", "image"), derived from the provider's `ProviderCapabilities`.
    pub features: Vec<String>,
    /// True for a low-cost / economy model (mini/nano/haiku/router tiers). Lets
    /// the UI group "cheap" models and pick a cheap default. Carried as a
    /// `"cheap"` entry in the proto `ModelInfo.features` list at the gRPC edge.
    pub cheap: bool,
}

/// Introspectable feature flags for a provider.
///
/// Added per `docs/capability-ownership-matrix.md` §G4 (shape adapted from
/// `OpenAI` Codex `ProviderCapabilities`, Apache-2.0). Lets the router and
/// capability-core policy gate modality/feature use *by querying the
/// provider* instead of hardcoding per-provider knowledge at the call site —
/// the prerequisite for clean multimodal routing (Phase 5) and routing
/// policies (Phase 2).
// Intended provider surface; constructed once routing/policy consumes it
// (Phase 2/5) — same "not yet consumed" convention as `ArtifactStore` above.
#[allow(dead_code)]
// Flags are independent capability bits mirroring the upstream Codex
// ProviderCapabilities shape; two-variant enums would add noise without value.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProviderCapabilities {
    /// Registry id this provider is addressed by (e.g. `azure-openai`).
    ///
    /// Declared here rather than inferred from the registration string so routing
    /// stops matching on hardcoded name literals. Adding a provider is then a
    /// declaration, not an edit to a `match` arm in the chain.
    pub provider_id: String,

    /// Additional `provider_hint` spellings that resolve to this provider.
    ///
    /// Replaces the fixed alias table that used to live in the chain. A hint is
    /// normalised (lowercase, `_`→`-`) before comparison, so only genuine
    /// synonyms belong here — not case or separator variants.
    pub aliases: Vec<String>,

    /// Which wire family this provider speaks, and therefore which model ids it
    /// could plausibly serve.
    pub model_family: ModelFamily,

    /// The strongest residency guarantee this provider's traffic honors.
    pub residency: Residency,

    /// When true, this provider serves *only* the models in its own catalog.
    ///
    /// The default (`false`) preserves the historical family-shape behavior: an
    /// `OpenAI`-shaped provider accepts any non-Claude model id, because the
    /// direct vendor APIs accept any published id and there is nothing
    /// authoritative to prune against.
    ///
    /// A sovereign provider must set this. Routing an unrecognised model to one
    /// either 404s or — far worse — gets silently served from a brokered upstream
    /// outside the residency boundary the tier was sold on.
    pub exclusive_catalog: bool,

    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_thinking: bool,
    pub supports_streaming: bool,
    pub supports_embeddings: bool,
    /// True only when the operator has independently verified that this exact
    /// provider/deployment contract honors Zero Data Retention. This defaults
    /// to false: geography, transport encryption, or a caller's `zdr` bit do
    /// not prove the downstream provider's retention behavior.
    pub supports_zdr: bool,
    /// Modality groups served, e.g. `["chat", "vision", "speech"]`.
    pub modalities: Vec<String>,
    pub max_context_tokens: u32,
    pub max_output_tokens: u32,
}

impl Default for ProviderCapabilities {
    /// Conservative chat-only baseline. Providers override `capabilities()`
    /// to advertise more — defaulting low means an unconfigured provider is
    /// never *assumed* to support a modality it cannot serve.
    fn default() -> Self {
        Self {
            provider_id: String::new(),
            aliases: Vec::new(),
            model_family: ModelFamily::OpenAiCompatible,
            // Deny-by-default: an undeclared provider gets the weakest residency,
            // so the registration gate refuses it rather than letting it inherit
            // an EU claim it never made.
            residency: Residency::Global,
            exclusive_catalog: false,
            supports_tools: false,
            supports_vision: false,
            supports_thinking: false,
            supports_streaming: true,
            supports_embeddings: false,
            supports_zdr: false,
            modalities: vec!["chat".to_owned()],
            max_context_tokens: 8_192,
            max_output_tokens: 4_096,
        }
    }
}

/// Which request/response wire family a provider speaks.
///
/// Replaces `matches!(provider_name, "anthropic" | "azure-anthropic")` in the
/// chain: the provider declares its own family, so a new provider is added by
/// declaring one rather than by extending a name-matching expression that every
/// future provider would also have to be threaded through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFamily {
    /// `OpenAI` chat-completions shape, including Azure `OpenAI` and any
    /// OpenAI-compatible third party.
    #[default]
    OpenAiCompatible,
    /// Anthropic Messages shape, direct or via Azure AI Foundry.
    Anthropic,
}

/// The strongest residency guarantee a provider's traffic honors.
///
/// Ordered weakest-to-strongest so a request can express a *minimum* and the
/// comparison is a plain `>=`. Distinct from `supports_zdr`: retention and
/// geography are independent axes. Azure `OpenAI` in an EU data zone with an
/// approved retention exception is both `Eu` and ZDR; Azure Foundry Claude today
/// is `Eu` without ZDR; a sovereign Norwegian deployment is `Norway`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Residency {
    /// No residency commitment — may be processed in any region worldwide.
    ///
    /// After the decision to drop Grok (see `PROVIDER_AND_PRIVACY_STRATEGY.md`
    /// §0.0) no configured provider should land here, which is why registration
    /// refuses it without an explicit opt-in.
    #[default]
    Global,
    /// ML processing committed to the EU/EEA.
    Eu,
    /// Processed and stored in Norway.
    Norway,
}

impl Residency {
    /// Classify one Azure resource's residency from its own signals.
    ///
    /// `Eu` requires positive evidence. Unknown is `Global`, not `Eu`: "we cannot
    /// prove this stays in the EU" and "this stays in the EU" are different claims
    /// and only one of them is true. An explicitly global deployment type is
    /// decisive over the region, because a Global deployment inside an EU region
    /// still processes inference worldwide.
    ///
    /// Takes one resource's signals only. An earlier version derived Azure Foundry
    /// Claude's residency from `AZURE_OPENAI_REGION`, which describes a *different*
    /// Azure resource — the checked-in configuration points them at two distinct
    /// hosts — so it would have declared Claude EU-resident on no evidence at all.
    #[must_use]
    pub fn classify(region_is_eu: bool, declared_global: bool) -> Self {
        if declared_global || !region_is_eu {
            Self::Global
        } else {
            Self::Eu
        }
    }

    /// Parse an operator-declared residency token.
    ///
    /// Unrecognised values return `None` so the caller can fail loud rather than
    /// silently downgrading to `Global` — a typo'd `MODEL_PLANE_..._RESIDENCY=noway`
    /// must not quietly become "no commitment".
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw
            .trim()
            .to_ascii_lowercase()
            .replace(['-', '_'], "")
            .as_str()
        {
            "global" | "worldwide" => Some(Self::Global),
            "eu" | "eea" | "euresident" => Some(Self::Eu),
            "norway" | "no" | "sovereign" => Some(Self::Norway),
            _ => None,
        }
    }

    /// Human-readable label for logs and the provenance receipt.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Eu => "eu",
            Self::Norway => "norway",
        }
    }
}

impl ProviderCapabilities {
    /// True if this provider advertises the named modality group.
    #[allow(dead_code)] // intended surface; consumed by router/policy (Phase 2/5)
    #[must_use]
    pub fn serves_modality(&self, modality: &str) -> bool {
        self.modalities.iter().any(|m| m == modality)
    }

    /// Project these capabilities onto chat-parity §2 feature-family strings so
    /// the client can gate the opt-in `features[]` per model. `usage` is always
    /// available (the gateway emits real token/latency); `citations` is
    /// gateway-side (Quarry/Data-Plane) so it is not advertised per-model here.
    #[must_use]
    pub fn feature_flags(&self) -> Vec<String> {
        let mut out = vec!["usage".to_owned()];
        if self.supports_thinking {
            out.push("reasoning".to_owned());
        }
        if self.supports_tools {
            out.push("tools".to_owned());
        }
        if self.supports_vision {
            out.push("vision".to_owned());
        }
        if self.serves_modality("image") {
            out.push("image".to_owned());
        }
        if self.supports_zdr {
            out.push("zdr".to_owned());
        }
        out
    }
}

/// Provider routing trait for inference backends.
#[async_trait::async_trait]
pub trait ProviderRouter: Send + Sync {
    /// Perform a unary inference call.
    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError>;

    /// Perform a streaming inference call, sending chunks to the returned receiver.
    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError>;

    /// Create an embedding vector. Providers that do not support embeddings
    /// should keep the default unsupported response.
    async fn create_embedding(&self, req: &EmbedRequest) -> Result<EmbedResponse, ProviderError> {
        Err(ProviderError::UnsupportedModel(format!(
            "embedding:{}",
            req.model
        )))
    }

    /// Return provider models known at startup.
    fn list_models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }

    /// Advertise this provider's feature/modality capabilities. Defaults to
    /// the conservative chat-only baseline; multimodal providers override.
    #[allow(dead_code)] // intended surface; consumed by router/policy (Phase 2/5)
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }
}

#[cfg(test)]
mod residency_tests {
    use super::Residency;

    /// The regression this classifier exists to prevent: promoting a resource to
    /// `Eu` on evidence that describes something else, or on no evidence.
    #[test]
    fn eu_requires_positive_evidence() {
        assert_eq!(Residency::classify(true, false), Residency::Eu);
        // Unknown region: cannot prove EU, so no commitment is claimed.
        assert_eq!(Residency::classify(false, false), Residency::Global);
    }

    /// A Global deployment inside an EU region still processes worldwide, so the
    /// declared type overrides the region rather than the other way round. If this
    /// inverted, the residency classification would promote exactly what the
    /// deployment-type startup gate refuses.
    #[test]
    fn declared_global_overrides_an_eu_region() {
        assert_eq!(Residency::classify(true, true), Residency::Global);
        assert_eq!(Residency::classify(false, true), Residency::Global);
    }

    #[test]
    fn ordering_supports_a_minimum_comparison() {
        assert!(Residency::Norway > Residency::Eu);
        assert!(Residency::Eu > Residency::Global);
        assert_eq!(Residency::default(), Residency::Global);
    }
}

#[cfg(test)]
mod capability_tests {
    use super::ProviderCapabilities;

    #[test]
    fn feature_flags_reflect_capability_bits() {
        let caps = ProviderCapabilities {
            supports_tools: true,
            supports_vision: true,
            supports_thinking: true,
            supports_zdr: true,
            modalities: vec!["chat".to_owned(), "image".to_owned()],
            ..ProviderCapabilities::default()
        };
        let flags = caps.feature_flags();
        assert!(flags.contains(&"usage".to_owned()));
        assert!(flags.contains(&"reasoning".to_owned()));
        assert!(flags.contains(&"tools".to_owned()));
        assert!(flags.contains(&"vision".to_owned()));
        assert!(flags.contains(&"image".to_owned()));
        assert!(flags.contains(&"zdr".to_owned()));
    }

    #[test]
    fn baseline_caps_only_advertise_usage() {
        let flags = ProviderCapabilities::default().feature_flags();
        assert_eq!(flags, vec!["usage".to_owned()]);
    }
}

/// Errors from provider operations.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("http error: {0}")]
    Http(String),

    #[error("rate limited: retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("provider unavailable: {0}")]
    Unavailable(String),

    #[error("all providers exhausted after {attempts} total attempts")]
    AllExhausted { attempts: u32 },

    #[allow(dead_code)]
    #[error("unsupported model: {0}")]
    UnsupportedModel(String),

    /// EU residency gate rejected this embedding before any network call
    /// (deny-by-default). The configured/requested region is outside the EU
    /// residency boundary and `MODEL_PLANE_ALLOW_NON_EU_EMBEDDING` is off.
    #[error("residency violation: {0}")]
    ResidencyViolation(String),

    /// A request required ZDR but no matching provider/deployment has an
    /// independently verified ZDR contract. Rejected before any provider call.
    #[error("zero data retention unavailable: {0}")]
    ZdrUnavailable(String),
}

/// Canonical EU Azure regions permitted to serve embeddings under the EU
/// residency boundary. Kept conservative (the regions where the embedding
/// deployments actually live / can live) rather than an exhaustive Azure list;
/// extend deliberately when a new EU deployment is provisioned.
pub(crate) const EU_AZURE_REGIONS: &[&str] = &[
    "swedencentral",
    "westeurope",
    "northeurope",
    "francecentral",
    "germanywestcentral",
    "norwayeast",
    "switzerlandnorth",
];

/// Normalize a region/endpoint token for matching: lowercase, strip spaces,
/// hyphens and underscores so `Sweden Central`, `sweden-central` and
/// `swedencentral` all collapse to the same canonical form.
#[must_use]
pub(crate) fn normalize_region_token(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .collect()
}

/// True when `region` names a region inside the EU residency boundary.
///
/// An empty region is treated as EU-safe: the caller expresses no preference,
/// so the configured (EU-by-default) deployment is used and the endpoint-level
/// check still applies. A non-empty, non-EU region is rejected by the gate.
#[must_use]
pub(crate) fn is_eu_region(region: &str) -> bool {
    let token = normalize_region_token(region);
    if token.is_empty() {
        return true;
    }
    EU_AZURE_REGIONS.contains(&token.as_str())
}

/// Best-effort classification of whether an Azure endpoint URL resolves to an
/// EU region. Azure `OpenAI` endpoints are typically
/// `https://<resource>.openai.azure.com` (region not in the host), so the
/// region is usually carried out-of-band (the request `region` field or an
/// explicit `AZURE_OPENAI_REGION`). This helper only flags an endpoint as
/// non-EU when a recognizable NON-EU region substring appears in the host
/// (e.g. `eastus2`, `westus`), so a region-less Azure host is NOT falsely
/// rejected — the explicit region/allow-flag remains the source of truth.
#[must_use]
pub(crate) fn endpoint_region_is_non_eu(endpoint: &str) -> bool {
    const NON_EU_REGION_MARKERS: &[&str] = &[
        "eastus",
        "westus",
        "centralus",
        "southcentralus",
        "northcentralus",
        "canadacentral",
        "canadaeast",
        "brazilsouth",
        "australiaeast",
        "australiasoutheast",
        "japaneast",
        "japanwest",
        "koreacentral",
        "southeastasia",
        "eastasia",
        "centralindia",
        "southindia",
        "uaenorth",
        "southafricanorth",
    ];
    let host = normalize_region_token(endpoint);
    NON_EU_REGION_MARKERS
        .iter()
        .any(|marker| host.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bearer_never_prints_its_value_even_inside_a_derived_debug() {
        // The whole point of the newtype: an `InferRequest` carries a live
        // per-user JWT, and the struct keeps `#[derive(Debug)]`. Any future
        // `?req` log line, panic message, or `#[instrument]` must not leak it.
        // A distinctive marker rather than a realistic JWT: the property under
        // test is "this value never appears in the output", and a token-shaped
        // literal would (rightly) trip the repo's secret scanner.
        let marker = "MUST-NOT-APPEAR-IN-ANY-LOG";
        let req = InferRequest {
            model: "claude-sonnet-4-6".to_owned(),
            caller_bearer: Bearer::new(marker),
            ..Default::default()
        };

        let printed = format!("{req:?}");
        assert!(
            !printed.contains(marker),
            "the bearer leaked through the derived Debug: {printed}"
        );
        assert!(printed.contains("[REDACTED]"));
        // Non-secret fields must still be debuggable.
        assert!(printed.contains("claude-sonnet-4-6"));
    }

    #[test]
    fn an_absent_bearer_is_distinguishable_from_a_present_one() {
        // "Was the token forwarded at all?" stays answerable without exposing it.
        assert_eq!(format!("{:?}", Bearer::default()), "Bearer([empty])");
        assert_eq!(format!("{:?}", Bearer::new("x")), "Bearer([REDACTED])");
    }

    #[test]
    fn default_capabilities_are_conservative_chat_only() {
        let caps = ProviderCapabilities::default();
        assert!(caps.serves_modality("chat"));
        assert!(!caps.serves_modality("vision"));
        assert!(!caps.supports_vision);
        assert!(!caps.supports_tools);
        assert!(!caps.supports_zdr);
        // streaming is the one safe-on default (virtually all chat providers).
        assert!(caps.supports_streaming);
    }

    #[test]
    fn capabilities_serialize_to_json() {
        let caps = ProviderCapabilities {
            supports_tools: true,
            supports_vision: true,
            modalities: vec!["chat".to_owned(), "vision".to_owned()],
            ..Default::default()
        };
        let json = serde_json::to_string(&caps).expect("serialize");
        assert!(json.contains("\"supports_vision\":true"));
        assert!(caps.serves_modality("vision"));
    }

    #[test]
    fn eu_regions_are_recognized_in_any_form() {
        for r in [
            "swedencentral",
            "Sweden Central",
            "sweden-central",
            "WESTEUROPE",
            "norway_east",
        ] {
            assert!(is_eu_region(r), "{r:?} should classify as EU");
        }
    }

    #[test]
    fn non_eu_regions_are_rejected() {
        for r in ["eastus2", "westus", "japaneast", "australiaeast"] {
            assert!(!is_eu_region(r), "{r:?} should classify as non-EU");
        }
    }

    #[test]
    fn empty_region_is_eu_safe() {
        // No preference → endpoint-level/config gate decides; treat as EU-safe.
        assert!(is_eu_region(""));
        assert!(is_eu_region("   "));
    }

    #[test]
    fn endpoint_non_eu_marker_detection() {
        assert!(endpoint_region_is_non_eu(
            "https://my-resource-eastus2.openai.azure.com"
        ));
        assert!(endpoint_region_is_non_eu(
            "https://acct.westus.cognitiveservices.azure.com"
        ));
        // Region-less Azure OpenAI hosts must NOT be falsely flagged non-EU.
        assert!(!endpoint_region_is_non_eu(
            "https://verevon.openai.azure.com"
        ));
        assert!(!endpoint_region_is_non_eu(
            "https://my-swedencentral-res.openai.azure.com"
        ));
    }
}
