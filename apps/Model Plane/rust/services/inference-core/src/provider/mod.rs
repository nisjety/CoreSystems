//! Provider routing traits and implementations.

use tracing::warn;

pub mod anthropic;
pub mod artifact_ref;
pub mod codex_subscription;
mod codex_subscription_tools;
pub mod doc_intel;
pub mod fallback;
pub mod intent;
pub mod language;
pub mod logprobs;
pub mod openai;
pub mod overflow;
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
    /// Minimum privacy tier every serving provider must satisfy. Providers
    /// whose derived tier is weaker are skipped in every chain path; when none
    /// remains the request fails closed with
    /// [`ProviderError::TierUnavailable`] naming the required tier — never a
    /// silent downgrade. `Unspecified` imposes no constraint and is
    /// byte-identical to pre-tier behavior.
    pub min_privacy_tier: PrivacyTier,
    /// chat-parity §2 function-calling: tools the model may call (empty = none).
    pub tools: Vec<ToolDefinition>,
    /// Tool selection policy: "auto" | "none" | "required" | a tool name.
    pub tool_choice: String,
    /// Requested minimum residency floor (e.g. "eu", "norway"). Enforced
    /// deny-by-default in `provider::fallback`: a provider whose declared
    /// [`Residency`] is weaker than this floor is skipped, mirroring how a
    /// non-ZDR provider is skipped when `zdr` is true. Empty means no floor.
    pub min_residency: String,
    /// Extended-thinking budget in tokens. 0 requests no thinking, which is the
    /// pre-existing behaviour byte-for-byte.
    ///
    /// Advisory: a provider forwards it only when the resolved model actually
    /// accepts a thinking parameter, because sending one to a model that does
    /// not is a hard 400 rather than a silent no-op. See
    /// `anthropic::supports_extended_thinking`.
    pub thinking_budget_tokens: i32,
    /// Optional speed tier for supported subscription models. It does not
    /// select a provider or reduce the requested reasoning effort.
    pub prefer_priority_service_tier: bool,
    /// Opaque Integration Core connection id for a user-owned subscription.
    /// This is a routing reference, never a ChatGPT OAuth credential. It is
    /// included in the prompt-cache scope so two connections cannot share a
    /// cached response or bypass the broker's per-connection audit boundary.
    pub subscription_connection_id: String,
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
    #[serde(skip)]
    pub compaction_summary: String,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// How sure the serving model itself was, token by token.
///
/// Summarized from provider logprobs by [`logprobs`]. `None` on a response or
/// final chunk means the provider does not report them (Anthropic never does)
/// or the model rejects the request parameter — "unknown", never "low".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenConfidence {
    /// Answer tokens the summary covers. Never 0 in a constructed value.
    pub token_count: u32,
    /// Mean natural-log probability over those tokens; `exp()` gives the
    /// geometric-mean per-token probability. Always <= 0.
    pub mean_logprob: f64,
    /// Tokens that carry a claim: content tokens which do not echo the
    /// question. 0 when the answer was entirely framing.
    pub claim_token_count: u32,
    /// The same mean over claim tokens only. Meaningless when
    /// `claim_token_count` is 0. See the proto field's doc for why this
    /// separates a fabrication from confident framing and the whole-answer
    /// mean does not.
    pub claim_mean_logprob: f64,
}

/// Unified inference response.
#[derive(Debug, Clone, Default)]
pub struct InferResponse {
    pub compaction_summary: String,
    pub request_id: String,
    pub content: String,
    pub model_used: String,
    pub stop_reason: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    /// chat-parity §2: tool calls the model requested (empty for a plain answer).
    pub tool_calls: Vec<ToolCall>,
    /// Registry id of the provider that actually served this response (e.g.
    /// "azure-openai"). Phase-4 provenance-receipt input; empty when unknown.
    pub provider_used: String,
    /// Residency label of the serving deployment ([`Residency::as_str`]).
    /// Disclosure only — reports the posture that was actually met. Empty when
    /// undeclared.
    pub residency: String,
    /// The serving model's own token-level certainty, when it reports logprobs.
    pub token_confidence: Option<TokenConfidence>,
    /// Tokens served from the provider's prompt cache (0 when not cached, or
    /// the provider does not support prompt caching). Already folded into
    /// `input_tokens` above (see `anthropic::total_input_tokens`); carried
    /// separately so cache-hit rate and cost savings are observable
    /// (cache-token telemetry, a native-compaction migration prerequisite).
    pub cache_read_input_tokens: i32,
    /// Tokens newly written to the provider's prompt cache by this request (0
    /// when not cached). Same fold-in relationship to `input_tokens` as
    /// `cache_read_input_tokens`.
    pub cache_creation_input_tokens: i32,
}

/// A single streaming chunk.
#[derive(Debug, Clone)]
pub struct InferChunk {
    pub compaction_summary: String,
    pub request_id: String,
    pub delta: String,
    pub done: bool,
    pub model_used: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    /// Why generation stopped. Populated on the final chunk only; see the
    /// proto field's doc for the full vocabulary, including
    /// `"stream_incomplete"` for a connection that broke before any proper
    /// termination signal arrived.
    pub stop_reason: String,
    /// Incremental extended-thinking text, when the model produced any.
    ///
    /// Kept separate from `delta` rather than merged: reasoning is not part of
    /// the answer, and a consumer that appended it would put the model's
    /// scratchpad into the user's reply. A chunk carries one or the other.
    pub reasoning_delta: String,
    /// Serving-provider provenance (populated on final chunks; same semantics
    /// as [`InferResponse::provider_used`]). Empty when unknown.
    pub provider_used: String,
    /// Residency label of the serving deployment (see [`InferResponse::residency`]).
    pub residency: String,
    /// Token-level certainty for the whole streamed answer, accumulated across
    /// the stream and populated on the FINAL chunk only — the same rule the
    /// token counts and provenance above follow, and for the same reason: it
    /// is a property of the completed answer, not of one delta.
    pub token_confidence: Option<TokenConfidence>,
    /// Tokens served from the provider's prompt cache, populated on the FINAL
    /// chunk only (same rule as `input_tokens`/`output_tokens` above). 0 when
    /// not cached, or the provider does not support prompt caching. Already
    /// folded into `input_tokens`; carried separately so cache-hit rate and
    /// cost savings are observable on the streaming path (previously the
    /// actual telemetry gap — see [`InferResponse::cache_read_input_tokens`]).
    pub cache_read_input_tokens: i32,
    /// Tokens newly written to the provider's prompt cache by this request,
    /// populated on the FINAL chunk only. 0 when not cached.
    pub cache_creation_input_tokens: i32,
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
    /// Minimum privacy tier the serving embedding provider must satisfy
    /// (same semantics as [`InferRequest::min_privacy_tier`]). There is no wire
    /// field on the embedding contract yet, so callers currently leave this at
    /// the default (`Unspecified`) — the gate exists so the enforcement path is
    /// shared, not so embeddings advertise tiers today.
    pub min_privacy_tier: PrivacyTier,
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
    /// Strongest privacy tier the owning provider can honor (Venice
    /// `model_spec.privacy` equivalent). `Unspecified` means the provider
    /// declares no posture.
    pub privacy_tier: PrivacyTier,
    /// Declared residency label ([`Residency::as_str`]); empty when undeclared.
    pub residency_label: String,
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
    ///
    /// This is where every Azure region belongs, **including `norwayeast`**. Being
    /// physically in Norway is not the distinction — a Microsoft-operated resource
    /// is a Tier B EU-resident provider no matter which EU region hosts it.
    Eu,
    /// Sovereign: processed and stored in Norway on Norwegian-operated
    /// infrastructure (Telenor AI Factory, or Bineric's own models over it).
    ///
    /// Reserved for the Tier A supplier in `PROVIDER_AND_PRIVACY_STRATEGY.md` §0.0.
    /// Do **not** classify an Azure `norwayeast` deployment here: it would claim
    /// sovereignty for a hyperscaler resource and let a Tier B provider serve
    /// traffic sold as Tier A. `is_eu_region` already maps every Azure region —
    /// Norwegian ones included — to [`Self::Eu`], which is correct.
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

/// Programmatic privacy tier of a provider/deployment (Venice-style).
///
/// Ordered weakest→strongest so a request expresses a MINIMUM and eligibility
/// is a plain `>=`. Combines the two independent axes — geography
/// ([`Residency`]) and retention (`supports_zdr`) — into one sellable posture:
/// `EuResident` demands `Residency::Eu`+, `ZdrContractual` additionally
/// demands a verified ZDR contract, and `Sovereign` demands Norwegian-operated
/// infrastructure WITH that ZDR contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyTier {
    /// No constraint expressed / no declared posture.
    #[default]
    Unspecified,
    /// No commitment beyond provider default.
    Global,
    /// ML processing committed to the EU/EEA.
    EuResident,
    /// EU-or-better residency plus an independently verified Zero-Data-
    /// Retention contract.
    ZdrContractual,
    /// Processed and stored in Norway on Norwegian-operated infrastructure,
    /// with a verified ZDR contract.
    Sovereign,
}

impl PrivacyTier {
    /// Numeric value matching the `model_plane.v1.PrivacyTier` proto enum.
    #[must_use]
    pub const fn as_wire_i32(self) -> i32 {
        match self {
            Self::Unspecified => 0,
            Self::Global => 1,
            Self::EuResident => 2,
            Self::ZdrContractual => 3,
            Self::Sovereign => 4,
        }
    }

    /// Inverse of [`Self::as_wire_i32`]. Unknown numerics (a NEWER client
    /// speaking a tier this build does not know) return `None` so callers can
    /// reject rather than silently treat an unrecognized requirement as none.
    #[must_use]
    pub fn from_wire(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Unspecified),
            1 => Some(Self::Global),
            2 => Some(Self::EuResident),
            3 => Some(Self::ZdrContractual),
            4 => Some(Self::Sovereign),
            _ => None,
        }
    }

    /// Human-readable label for logs, typed errors, and the provenance receipt.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Global => "global",
            Self::EuResident => "eu_resident",
            Self::ZdrContractual => "zdr_contractual",
            Self::Sovereign => "sovereign",
        }
    }

    /// Derive the strongest tier a provider can honor from its declared
    /// capabilities. Geography comes from `residency`; the retention axis from
    /// `supports_zdr`. A Norwegian-operated resource WITHOUT a ZDR attestation
    /// classifies `EuResident` on purpose: claiming sovereignty without the
    /// retention contract would sell a guarantee the deployment does not make.
    #[must_use]
    pub fn classify(caps: &ProviderCapabilities) -> Self {
        if !caps.supports_zdr {
            return match caps.residency {
                Residency::Global => Self::Global,
                Residency::Eu | Residency::Norway => Self::EuResident,
            };
        }
        match caps.residency {
            // Sovereignty is Norway-specific per the pinned contract: EU
            // residency with a verified ZDR contract stays `ZdrContractual`,
            // because geography alone is not sovereignty.
            Residency::Norway => Self::Sovereign,
            Residency::Eu | Residency::Global => Self::ZdrContractual,
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

    /// An Azure Norwegian region is EU-resident, not sovereign. Classifying it as
    /// `Norway` would claim Tier A sovereignty for a Microsoft-operated resource
    /// and let a Tier B provider serve traffic sold as Tier A.
    #[test]
    fn an_azure_norwegian_region_classifies_as_eu_not_sovereign() {
        assert!(
            super::is_eu_region("norwayeast"),
            "norwayeast must be recognised as an EU region"
        );
        assert_eq!(
            Residency::classify(super::is_eu_region("norwayeast"), false),
            Residency::Eu,
            "an Azure Norwegian region is EU-resident; Norway is reserved for \
             Norwegian-operated sovereign infrastructure"
        );
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

#[cfg(test)]
mod privacy_tier_tests {
    use super::{PrivacyTier, ProviderCapabilities, Residency};

    /// The tier ladder is the sellable contract: a request expresses a MINIMUM
    /// and eligibility is `>=`. Reordering these values would silently reprice
    /// every customer's posture.
    #[test]
    fn tiers_are_ordered_weakest_to_strongest() {
        assert!(PrivacyTier::Global > PrivacyTier::Unspecified);
        assert!(PrivacyTier::EuResident > PrivacyTier::Global);
        assert!(PrivacyTier::ZdrContractual > PrivacyTier::EuResident);
        assert!(PrivacyTier::Sovereign > PrivacyTier::ZdrContractual);
        assert_eq!(PrivacyTier::default(), PrivacyTier::Unspecified);
    }

    /// The wire enum is pinned by the proto contract; the frontend and
    /// capability-core both encode these numerics independently.
    #[test]
    fn wire_values_match_the_pinned_proto_contract() {
        for (value, tier) in [
            (0, PrivacyTier::Unspecified),
            (1, PrivacyTier::Global),
            (2, PrivacyTier::EuResident),
            (3, PrivacyTier::ZdrContractual),
            (4, PrivacyTier::Sovereign),
        ] {
            assert_eq!(tier.as_wire_i32(), value);
            assert_eq!(PrivacyTier::from_wire(value), Some(tier));
        }
    }

    /// An unknown numeric must NOT collapse to "no constraint" — that would let
    /// a newer client's stronger requirement be honored by an older build as if
    /// it had asked for nothing. Fail loud instead.
    #[test]
    fn unknown_wire_values_fail_closed() {
        for value in [-1, 5, 42, i32::MAX] {
            assert_eq!(
                PrivacyTier::from_wire(value),
                None,
                "wire value {value} must be rejected, not downgraded"
            );
        }
    }

    #[test]
    fn labels_use_snake_case_for_logs_and_receipts() {
        assert_eq!(PrivacyTier::Unspecified.label(), "unspecified");
        assert_eq!(PrivacyTier::Global.label(), "global");
        assert_eq!(PrivacyTier::EuResident.label(), "eu_resident");
        assert_eq!(PrivacyTier::ZdrContractual.label(), "zdr_contractual");
        assert_eq!(PrivacyTier::Sovereign.label(), "sovereign");
    }

    fn caps(residency: Residency, supports_zdr: bool) -> ProviderCapabilities {
        ProviderCapabilities {
            residency,
            supports_zdr,
            ..ProviderCapabilities::default()
        }
    }

    /// The full mapping table. Two rows are deliberate traps:
    /// Norway WITHOUT ZDR classifies `EuResident` (geography alone is not
    /// sovereignty), and Global WITH ZDR classifies `ZdrContractual` (a retention
    /// contract does not relocate processing).
    #[test]
    fn classification_combines_geography_and_retention() {
        // No ZDR attestation: geography alone caps at EU residency.
        assert_eq!(
            PrivacyTier::classify(&caps(Residency::Global, false)),
            PrivacyTier::Global
        );
        assert_eq!(
            PrivacyTier::classify(&caps(Residency::Eu, false)),
            PrivacyTier::EuResident
        );
        assert_eq!(
            PrivacyTier::classify(&caps(Residency::Norway, false)),
            PrivacyTier::EuResident,
            "a Norwegian region without a verified ZDR contract is EU-resident, \
             never Sovereign"
        );
        // ZDR verified: retention satisfied, geography decides the rest.
        assert_eq!(
            PrivacyTier::classify(&caps(Residency::Global, true)),
            PrivacyTier::ZdrContractual,
            "a ZDR contract without EU residency stays at ZdrContractual"
        );
        assert_eq!(
            PrivacyTier::classify(&caps(Residency::Eu, true)),
            PrivacyTier::ZdrContractual,
            "EU residency plus a ZDR contract is ZdrContractual — SOVEREIGN is \
             Norway-only per the pinned contract"
        );
        assert_eq!(
            PrivacyTier::classify(&caps(Residency::Norway, true)),
            PrivacyTier::Sovereign
        );
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

    /// The prompt exceeded the provider's input limit. Distinguished from
    /// [`ProviderError::Http`] so callers can shed history and retry instead of
    /// re-deriving the intent from provider prose — see
    /// [`crate::provider::overflow`] for why that classification lives here and
    /// not downstream.
    ///
    /// `detail` is the provider's original `"<status>: <body>"` text, unchanged.
    #[error("prompt too long for the provider: {detail}")]
    TooLong { detail: String },

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

    /// A request required a minimum privacy tier no matching provider can
    /// honor (deny-by-default, before any provider call). The message names
    /// the REQUIRED tier so callers see exactly what could not be met — a
    /// downgrade is never silent.
    #[error("required privacy tier unavailable: {0}")]
    TierUnavailable(String),
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

    // --- tool_parameters: a malformed schema must be LOUD, never silent -------

    /// The bug this replaces: a bare `unwrap_or_else` turned an unparseable
    /// schema into `{"type":"object","properties":{}}`, which tells the provider
    /// "this function accepts anything". The model then invents argument names,
    /// the executor rejects them, and the visible symptom is a tool that
    /// mysteriously never works — with nothing anywhere naming the real cause.
    #[test]
    fn an_unparseable_schema_still_degrades_but_is_reported() {
        let open = serde_json::json!({ "type": "object", "properties": {} });
        // Invalid JSON.
        assert_eq!(tool_parameters("broken_tool", "{not json"), open);
        // Valid JSON that is not an object — a schema has to be an object.
        assert_eq!(tool_parameters("array_tool", "[1,2,3]"), open);
        assert_eq!(tool_parameters("string_tool", "\"nope\""), open);
        assert_eq!(tool_parameters("null_tool", "null"), open);
    }

    /// An ABSENT schema is a legitimate "this tool takes no arguments" — the
    /// same output, but not a fault, and it must not be reported as one or the
    /// warning becomes noise every caller learns to ignore.
    #[test]
    fn an_absent_schema_is_not_treated_as_a_malformation() {
        let open = serde_json::json!({ "type": "object", "properties": {} });
        for empty in ["", "   ", "\n"] {
            assert_eq!(tool_parameters("no_args_tool", empty), open);
        }
    }

    /// A good schema passes through byte-for-byte. Degrading a valid schema
    /// would be strictly worse than the bug being fixed.
    #[test]
    fn a_valid_schema_is_passed_through_untouched() {
        let raw =
            r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}"#;
        let parsed = tool_parameters("knowledge_search", raw);
        assert_eq!(
            parsed,
            serde_json::from_str::<serde_json::Value>(raw).unwrap()
        );
        assert_eq!(parsed["required"][0], "query");
    }

    /// The log line has to say WHICH tool, or an operator with twenty tools
    /// learns only that one of them is broken.
    #[test]
    fn the_kind_of_the_wrong_value_is_named() {
        assert_eq!(json_kind(&serde_json::json!([])), "array");
        assert_eq!(json_kind(&serde_json::json!("x")), "string");
        assert_eq!(json_kind(&serde_json::json!(null)), "null");
        assert_eq!(json_kind(&serde_json::json!(1)), "number");
        assert_eq!(json_kind(&serde_json::json!(true)), "bool");
        assert_eq!(json_kind(&serde_json::json!({})), "object");
    }
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

/// Parse a tool's declared JSON-Schema parameters, or fall back to an open
/// object — **loudly**.
///
/// # Why the fallback is kept, and why it must not be silent
///
/// This used to be a bare `.unwrap_or_else(...)` producing
/// `{"type":"object","properties":{}}` with no signal at all. That is the worst
/// possible failure to hide: an open schema tells the provider "this function
/// takes anything", so the model invents argument names, the call reaches an
/// executor that rejects it, and the only visible symptom is a tool that
/// mysteriously never works. The schema was malformed the whole time and nothing
/// said so.
///
/// The fallback itself stays, deliberately. Rejecting the request would fail the
/// entire turn because *one* of possibly twenty tools has a bad schema — a
/// caller's authoring mistake would become an outage. Degrading one tool and
/// naming it is the proportionate response.
///
/// Callers that own the schema (`builtin_tool_defs`, `offered_tool_defs`) should
/// never trip this; a client-declared or MCP-registered tool can.
///
/// Lives here rather than in one provider because BOTH the `OpenAI` and Anthropic
/// paths had the same silent `unwrap_or_else` — the identical bug twice is what
/// a shared concern looks like before it is shared.
pub(crate) fn tool_parameters(tool_name: &str, parameters_json: &str) -> serde_json::Value {
    let trimmed = parameters_json.trim();
    // An absent schema is a legitimate "no arguments", not a malformation. Only
    // a *present but unparseable* one is a fault worth reporting.
    if trimmed.is_empty() {
        return serde_json::json!({ "type": "object", "properties": {} });
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) if value.is_object() => value,
        Ok(other) => {
            warn!(
                tool = tool_name,
                kind = json_kind(&other),
                "tool parameter schema is not a JSON object; the provider will be told this \
                 tool accepts any arguments, so its calls will likely be rejected downstream"
            );
            serde_json::json!({ "type": "object", "properties": {} })
        }
        Err(error) => {
            warn!(
                tool = tool_name,
                %error,
                "tool parameter schema is not valid JSON; the provider will be told this tool \
                 accepts any arguments, so its calls will likely be rejected downstream"
            );
            serde_json::json!({ "type": "object", "properties": {} })
        }
    }
}

/// Name of a JSON value's type, for a log line that says what arrived instead of
/// an object.
fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
