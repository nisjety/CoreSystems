//! Sequential fallback chain — tries providers in order with bounded retries.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use arc_swap::ArcSwap;
use mp_contracts::model_entitlement::Reason;
use tokio::sync::mpsc;
use tracing::{info, warn, Instrument};

use super::catalogue_overlay::{self, OrgOverlay};
use super::gemini::GeminiProvider;
use super::policy_client::PolicyClient;
use super::role::{self, RoleCall};
use super::routing_policy::{RolePurpose, RoutingPolicy};
use super::timing::{CallClock, StreamClock};
use super::{
    anthropic::AnthropicProvider, codex_subscription::CodexSubscriptionProvider,
    effort, endpoint_region_is_non_eu, intent, is_eu_region, model_accepts_images,
    normalize_region_token, openai::OpenAiProvider, EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse,
    ModelFamily, ModelInfo, PrivacyTier, ProviderCapabilities, ProviderError, ProviderRouter,
    Residency,
};
use crate::cache::PromptCache;
use crate::config::{EntitlementConfig, InferenceConfig, VertexGeminiConfig};
use crate::entitlement::{EntitlementGate, RequestAccess};

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
    /// Resolves the caller's model entitlement (ADR-0005). Off unless
    /// `MODEL_ENTITLEMENT_MODE` says otherwise; the handlers ask it, and the
    /// chain applies the answer a request carries.
    entitlement: Arc<EntitlementGate>,
    /// Failure bench and user-route guard (`INFERENCE_PROVIDER_BENCHING`). `None` when the switch is off:
    /// the walk then neither benches nor guards and is unchanged.
    bench: Option<Arc<super::health::BenchBoard>>,
    /// Whether every fallback hop stays at or above the privacy tier of the first eligible provider
    /// (`INFERENCE_SAME_TIER_FALLBACK`). Off, a hop may land on a lower tier, as it does today.
    same_tier: bool,
}

/// The catalogue overlay that refuses a call before anything is sent, as admission checks them: the platform overlay
/// first, then the caller's organisation overlay (V3-inf-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidingOverlay {
    /// The platform catalogue overlay (`INFERENCE_CATALOGUE_OVERLAY`).
    Platform,
    /// The caller's organisation overlay (`INFERENCE_ORG_OVERLAY`).
    Organisation,
}

/// Switch for the route note (WS27-S7): what the budget did to a Verevon call, carried on the wire. Off
/// unless the literal `true`, read once at startup, as the other inference switches are.
const ROUTE_NOTE_ENV: &str = "INFERENCE_ROUTE_NOTE";

/// Whether an explicit env value turns the route note on. Pure so the policy is unit-testable without
/// touching the process environment.
fn route_note_enabled_from(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// Whether the route note is on for this process.
fn route_note_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| route_note_enabled_from(std::env::var(ROUTE_NOTE_ENV).ok().as_deref()))
}

/// What the budget did to one Verevon-routed call (WS27-S7), kept beside its response. Closed values and
/// booleans only, never a price or a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteNote {
    pub budget_posture: intent::BudgetPosture,
    /// Set only when the posture moved the model one tier cheaper: the same policy at a healthy budget
    /// would have answered with another model.
    pub tier_shifted: bool,
    /// Set when an exhausted budget forced the cheap fallback model.
    pub cheap_fallback_forced: bool,
    /// The effort level the budget clamped the call to, when it clamped it.
    pub applied_effort: Option<effort::Level>,
    /// The role table's decision, for a `verevon-role` call (EP-13 S4). `None` for every other call.
    pub role: Option<role::RoleDecision>,
    /// The Verevon mode the intent layer resolved the call under. `None` when it resolved no mode,
    /// as for a call that named a pinned model.
    pub mode: Option<intent::VerevonMode>,
    /// The complexity the intent layer estimated for the call, alongside `mode`. `None` when it
    /// resolved no mode.
    pub complexity: Option<intent::Complexity>,
    /// Why the intent layer chose the model (see [`intent::Decision::reason`]). Empty when it
    /// resolved no mode, and when a catalogue overlay moved the call off the model it chose.
    pub reason: &'static str,
}

impl RouteNote {
    /// The note of a call the budget never priced: the posture is unknown, and nothing was
    /// moved or forced. A role call carries it so its decision has a note to ride on.
    const fn unbudgeted() -> Self {
        Self {
            budget_posture: intent::BudgetPosture::Unknown,
            tier_shifted: false,
            cheap_fallback_forced: false,
            applied_effort: None,
            role: None,
            mode: None,
            complexity: None,
            reason: "",
        }
    }
}

/// The route note with a role decision recorded on it. A role call always carries a note, even
/// when the budget note is switched off, because its decision is what the note exists to show.
/// Every other call keeps its note as it was.
fn attach_role(note: Option<RouteNote>, decision: Option<role::RoleDecision>) -> Option<RouteNote> {
    match decision {
        None => note,
        Some(decision) => Some(RouteNote {
            role: Some(decision),
            ..note.unwrap_or_else(RouteNote::unbudgeted)
        }),
    }
}

/// The note for a decision the intent layer resolved. `tier_shifted` is read against the same policy at a
/// healthy budget, so a mode the caller chose is never reported as a budget move.
fn budget_route_note(
    policy: &RoutingPolicy,
    decision: &intent::Decision,
    applied_effort: Option<effort::Level>,
) -> RouteNote {
    let healthy = intent::choose(
        policy,
        decision.mode,
        decision.complexity,
        intent::BudgetPosture::Healthy,
    );
    RouteNote {
        budget_posture: decision.posture,
        tier_shifted: decision.posture == intent::BudgetPosture::Constrained
            && healthy != decision.model,
        cheap_fallback_forced: decision.posture == intent::BudgetPosture::Exhausted,
        applied_effort,
        role: None,
        mode: Some(decision.mode),
        complexity: Some(decision.complexity),
        reason: decision.reason(),
    }
}

/// The note of an intent call a catalogue overlay moved off the model the intent layer chose (V3-inf-1). The budget
/// reasons describe the chosen model, which is not the one served, so the note names no budget move and no reason.
/// The mode, the complexity, the posture and the effort the budget applied still describe the call.
fn overlay_route_note(
    decision: &intent::Decision,
    applied_effort: Option<effort::Level>,
) -> RouteNote {
    RouteNote {
        budget_posture: decision.posture,
        tier_shifted: false,
        cheap_fallback_forced: false,
        applied_effort,
        role: None,
        mode: Some(decision.mode),
        complexity: Some(decision.complexity),
        reason: "",
    }
}

impl From<RouteNote> for mp_contracts::model_plane::v1::RouteNote {
    fn from(note: RouteNote) -> Self {
        let budget_posture = match note.budget_posture {
            intent::BudgetPosture::Healthy => "healthy",
            intent::BudgetPosture::Constrained => "constrained",
            intent::BudgetPosture::Exhausted => "exhausted",
            intent::BudgetPosture::Unknown => "unknown",
        };
        Self {
            budget_posture: budget_posture.to_owned(),
            tier_shifted: note.tier_shifted,
            cheap_fallback_forced: note.cheap_fallback_forced,
            applied_effort: note
                .applied_effort
                .map_or_else(String::new, |level| level.as_str().to_owned()),
            role: note
                .role
                .and_then(|decision| decision.table_entry)
                .map_or_else(String::new, |purpose| purpose.as_str().to_owned()),
            role_source: note
                .role
                .map_or_else(String::new, |decision| decision.source.as_str().to_owned()),
            verevon_mode: note
                .mode
                .map_or_else(String::new, |mode| mode.as_str().to_owned()),
            complexity: note
                .complexity
                .map_or_else(String::new, |complexity| complexity.as_str().to_owned()),
            reason: note.reason.to_owned(),
        }
    }
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
    codex_subscription: String,
}

impl Default for ProviderDefaults {
    fn default() -> Self {
        Self {
            azure_openai: AZURE_MODEL_ROUTER.to_owned(),
            azure_anthropic: super::anthropic::DEFAULT_AZURE_ANTHROPIC_MODEL.to_owned(),
            codex_subscription: String::new(),
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
            codex_subscription: cfg
                .codex_subscription_models
                .first()
                .cloned()
                .unwrap_or_default(),
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
pub(crate) fn is_unspecified_model(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    // Empty / "default", plus any Verevon intent id. The intent layer normally
    // rewrites a `verevon-*` id to a concrete model before the provider loop;
    // treating it as unspecified here is the safety net for when the intent
    // layer is disabled — the id resolves to the provider default instead of
    // being sent verbatim (which would 404 the deployment).
    m.is_empty() || m == "default" || intent::parse_mode(model).is_some()
}

/// The model a request naming `model` is served as once `with_current_model` has run: the
/// successor of a retired Claude id, and `model` itself otherwise. The catalogue overlay checks
/// this id too, so a hidden successor cannot be reached by naming the retired id.
fn served_model(model: &str) -> &str {
    super::model_record::retired_successor(super::effort::Family::Anthropic, model).unwrap_or(model)
}

/// Whether the caller's organisation overlay hides `model`, or the model that would serve it (the successor of a
/// retired Claude id). An unspecified id names no model, so it is not judged here: a walk judges the provider
/// default it resolves to, on the model that provider would really serve.
fn org_hides(org: Option<&OrgOverlay>, model: &str) -> bool {
    org.is_some_and(|org| {
        !is_unspecified_model(model) && (org.is_hidden(model) || org.is_hidden(served_model(model)))
    })
}

/// True for an Anthropic-family model id (the only models the Anthropic-shaped
/// providers can serve). Used to keep `claude-*` requests off the OpenAI/Azure
/// chat-completions surface (which would 404 the deployment) and vice-versa.
fn is_anthropic_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("claude")
}

/// True for a Gemini model id. Gemini ids are served only by the Vertex provider, so they
/// never reach an OpenAI-shaped surface, which would answer them with a 404.
fn is_gemini_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("gemini")
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
    min_privacy_tier == PrivacyTier::Unspecified || PrivacyTier::classify(caps) >= min_privacy_tier
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

/// The typed exhaustion error for a request that carries images whose entire
/// matching chain was skipped for being unable to read them. Content-free: it
/// names the cause, never the images.
fn vision_unsupported_error() -> ProviderError {
    ProviderError::VisionUnsupported(
        "no configured provider serves this model with image input".to_owned(),
    )
}

/// A refusal's reset time for the log line: RFC 3339 UTC, or empty if unknown.
fn reset_label(resets_at: Option<chrono::DateTime<chrono::Utc>>) -> String {
    resets_at
        .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
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
#[derive(Debug, Default, Clone)]
struct ThrottleState {
    /// Shortest `retry_after_ms` any throttled provider asked for — the soonest
    /// moment a retry could plausibly succeed.
    soonest_retry_ms: Option<u64>,
    /// The first `TooLong` detail any provider returned during the walk.
    ///
    /// Recorded so exhaustion can surface the TYPED overflow instead of a
    /// generic `AllExhausted`. Until this field existed, `classify_http_failure`
    /// produced `ProviderError::TooLong` at five call sites and every chain
    /// discarded it in a `Err(e) => warn!` arm — so `grpc.rs`'s `too_long_status`
    /// (and its `x-mp-provider-error: too_long` trailer, and the gateway's
    /// context-length recovery keyed on it) had zero production callers. The
    /// walk still CONTINUES past a TooLong on purpose: a later provider with a
    /// larger window is the recovery path, and only exhaustion reports it.
    too_long_detail: Option<String>,
    /// The first `RequestRejected` detail any provider returned during the walk.
    ///
    /// A provider that refused the request is not retried, but the walk still asks the next
    /// provider or model, which may serve the same request. When nothing else was learned,
    /// exhaustion names the refusal, since the same request would be refused again.
    rejected_detail: Option<String>,
    /// The first final subscription outcome of the walk: a
    /// [`ProviderError::SubscriptionRefused`], or a
    /// [`ProviderError::SubscriptionFailed`] turn.
    ///
    /// A refusal is about the connected ChatGPT account (its usage limit, its
    /// sign-in, its upstream), so no retry, wait or other provider can answer
    /// it. A failed turn has already been run by the broker, so a retry would
    /// spend another turn of the plan's quota on what is mostly a repeatable
    /// failure. Either ends the walk, and exhaustion reports it ahead of
    /// everything else. It is deliberately not throttling — `throttled()`
    /// ignores it, so there is no second pass and no wait.
    refused: Option<ProviderError>,
    /// The first reason the caller's entitlement skipped a candidate. Set only
    /// under `enforce`; with no attempt made it ends the walk as
    /// `model_not_entitled`, named before any zero-retention, residency or tier
    /// refusal.
    entitlement_denial: Option<Reason>,
    /// A candidate that cleared every other gate was skipped only because the
    /// request carries images and it cannot read them. With no attempt made it
    /// ends the walk as `vision_unsupported`, ahead of the privacy, retention
    /// and residency refusals, which a candidate that reached this gate passed.
    vision_skipped: bool,
    /// A candidate the platform catalogue overlay or the caller's organisation overlay hides, skipped before any
    /// provider was asked. A named model is refused at admission, so only a model the intent layer, the role table,
    /// the tool ladder or an automatic provider default chose reaches the walk hidden. With no attempt made it ends
    /// the walk as `model_hidden`, ahead of the posture refusals.
    hidden_skipped: bool,
    /// The privacy tier of the first provider this call found eligible, set only under
    /// `INFERENCE_SAME_TIER_FALLBACK`. It holds for the whole call: every rung of the ladder and both
    /// passes, so a pass reset keeps it ([`ThrottleState::next_pass`]).
    anchor_tier: Option<PrivacyTier>,
    /// The earliest end of a bench this pass skipped a provider for, set only under
    /// `INFERENCE_PROVIDER_BENCHING`. Only a provider that passed every gate is counted here, so with no
    /// attempt made it ends the walk as `providers_benched` (retry later), ahead of the posture refusals.
    /// Unlike `anchor_tier` it is reset with the pass.
    benched_until: Option<tokio::time::Instant>,
}

impl ThrottleState {
    /// Notes that a provider was skipped for a bench ending at `until`, keeping the earliest end.
    fn note_benched(&mut self, until: tokio::time::Instant) {
        self.benched_until = Some(match self.benched_until {
            Some(current) => current.min(until),
            None => until,
        });
    }

    /// Clears what one pass learned before the second pass, and keeps the call's tier anchor.
    fn next_pass(&mut self) {
        let anchor = self.anchor_tier;
        *self = Self::default();
        self.anchor_tier = anchor;
    }

    fn record_refusal(&mut self, refusal: ProviderError) {
        if self.refused.is_none() {
            self.refused = Some(refusal);
        }
    }

    /// Whether a subscription refusal ended the walk.
    fn refused(&self) -> bool {
        self.refused.is_some()
    }

    fn record(&mut self, retry_after_ms: u64) {
        self.soonest_retry_ms = Some(match self.soonest_retry_ms {
            Some(current) => current.min(retry_after_ms),
            None => retry_after_ms,
        });
    }

    fn record_too_long(&mut self, detail: &str) {
        if self.too_long_detail.is_none() {
            self.too_long_detail = Some(detail.to_owned());
        }
    }

    fn record_rejected(&mut self, detail: &str) {
        if self.rejected_detail.is_none() {
            self.rejected_detail = Some(detail.to_owned());
        }
    }

    fn throttled(&self) -> bool {
        self.soonest_retry_ms.is_some()
    }

    /// The wait to honor before retrying the chain, or `None` when the provider's
    /// own retry-after exceeds what an interactive request should absorb.
    fn affordable_wait(&self) -> Option<Duration> {
        let wait = Duration::from_millis(self.soonest_retry_ms?);
        (wait <= rate_limit_max_wait()).then_some(wait)
    }

    /// The error a throttled, exhausted chain should return.
    ///
    /// `RateLimited` rather than `AllExhausted` on purpose: it carries the
    /// retry-after, so a caller can say "try again in about a minute" instead of
    /// reporting a generic failure for something that is neither permanent nor
    /// the user's fault.
    fn exhausted_error(&self, attempts: u32) -> ProviderError {
        // A subscription refusal outranks everything: it is the one outcome
        // that names what the user must do (wait for the reset, reconnect),
        // and no other provider was, or may be, asked instead.
        if let Some(refused) = &self.refused {
            return refused.clone();
        }
        // Priority order is deliberate. RateLimited first: a throttled provider
        // might genuinely serve this exact prompt after the wait, so "try again
        // shortly" is still the true story. TooLong next: it is deterministic —
        // the same prompt overflows the same providers every time — so once no
        // retry can help, the caller needs the typed overflow to trigger
        // compaction, not a generic exhaustion. RequestRejected after that: a provider refused
        // the request itself, and a retry would hear the same refusal. AllExhausted only when
        // the walk learned nothing more specific.
        match (&self.soonest_retry_ms, &self.too_long_detail, &self.rejected_detail) {
            (Some(retry_after_ms), _, _) => ProviderError::RateLimited {
                retry_after_ms: *retry_after_ms,
            },
            (None, Some(detail), _) => ProviderError::TooLong {
                detail: detail.clone(),
            },
            (None, None, Some(detail)) => ProviderError::RequestRejected {
                detail: detail.clone(),
            },
            (None, None, None) => ProviderError::AllExhausted { attempts },
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
        // "Auto" resolves to a platform default that every provider may take, except
        // Gemini: its catalogue is opt-in by model id, and an Auto prompt must not leave
        // for Vertex just because the chain tried it first.
        return caps.model_family != ModelFamily::Gemini;
    }
    let family_matches = match caps.model_family {
        ModelFamily::Anthropic => is_anthropic_model(model),
        ModelFamily::OpenAiCompatible => !is_anthropic_model(model) && !is_gemini_model(model),
        ModelFamily::Gemini => is_gemini_model(model),
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

/// Validates `req.min_residency` before any provider is tried. Fails loud on
/// an unrecognized token rather than silently treating a typo as "no floor"
/// — the same reasoning as `Residency::parse`'s own doc comment. Empty is
/// always valid (no floor requested).
fn validate_min_residency(req: &InferRequest) -> Result<(), ProviderError> {
    let requested = req.min_residency.trim();
    if requested.is_empty() || Residency::parse(requested).is_some() {
        return Ok(());
    }
    Err(ProviderError::ResidencyViolation(format!(
        "unrecognized min_residency token `{requested}`"
    )))
}

/// Whether `caps_residency` fails to meet the request's residency floor.
/// Called once per candidate provider, the way the ZDR gate already is.
///
/// An unparseable token cannot reach here in production: `infer`/`infer_stream`
/// both call [`validate_min_residency`] before trying any provider. Treating it
/// as "no floor" here rather than panicking is a defensive fallback only, not
/// a second enforcement path.
fn residency_gate_blocks(req: &InferRequest, caps_residency: Residency) -> bool {
    let requested = req.min_residency.trim();
    if requested.is_empty() {
        return false;
    }
    Residency::parse(requested).is_some_and(|required| caps_residency < required)
}

impl FallbackChain {
    /// Build a fallback chain from configuration. The Vertex Gemini settings are read from
    /// the environment, so boot code calls this. Tests call
    /// [`Self::from_config_with_gemini`] and choose those settings themselves.
    pub fn from_config(cfg: &InferenceConfig) -> Self {
        Self::from_config_with_gemini(cfg, &VertexGeminiConfig::from_env())
    }

    /// Build a fallback chain from configuration with the Vertex Gemini settings given.
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
    pub fn from_config_with_gemini(cfg: &InferenceConfig, gemini: &VertexGeminiConfig) -> Self {
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
            if matches!(name.as_str(), "codex-subscription" | "chatgpt-codex")
                && providers
                    .iter()
                    .any(|(registered, _)| registered == "openai-codex-subscription")
            {
                continue;
            }
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
                "vertex-gemini" | "gemini" => {
                    // Refusals are logged inside `from_config`; the chain just goes without.
                    if !providers
                        .iter()
                        .any(|(registered, _)| registered == "vertex-gemini")
                    {
                        if let Some(p) = GeminiProvider::from_config(gemini) {
                            providers.push(("vertex-gemini".to_owned(), Arc::new(p)));
                            info!(provider = "vertex-gemini", "provider registered");
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
                "openai-codex-subscription" | "codex-subscription" | "chatgpt-codex" => {
                    if let (Some(url), Some(key)) = (
                        &cfg.codex_subscription_integration_core_url,
                        &cfg.codex_subscription_internal_api_key,
                    ) {
                        match CodexSubscriptionProvider::new(
                            url.clone(),
                            key.clone(),
                            cfg.codex_subscription_models.clone(),
                        ) {
                            Ok(provider) => {
                                providers.push((
                                    "openai-codex-subscription".to_owned(),
                                    Arc::new(provider),
                                ));
                                info!(
                                    provider = "openai-codex-subscription",
                                    "subscription provider registered"
                                );
                            }
                            Err(error) => warn!(
                                provider = "openai-codex-subscription",
                                error = %error,
                                "subscription provider configuration rejected"
                            ),
                        }
                    } else {
                        warn!(
                            provider = "openai-codex-subscription",
                            "subscription provider requested but Integration Core URL or service key is absent"
                        );
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
        // Request-side enforcement now exists too (`InferRequest.min_residency`,
        // gated in `infer_one_model`/`stream_one_model` via `residency_gate_blocks`
        // exactly the way a non-ZDR provider is skipped). This boot-time block
        // stays disclosure-only on purpose: it reports the STATIC fleet's posture
        // once at startup for operator visibility, independent of whether any
        // given request asks for a floor at all.
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
            entitlement: Arc::new(EntitlementGate::new(&EntitlementConfig::from_env())),
            bench: None,
            same_tier: false,
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
            entitlement: Arc::new(EntitlementGate::off()),
            bench: None,
            same_tier: false,
        }
    }

    /// Replace the entitlement gate (test/builder helper).
    #[allow(dead_code)]
    #[must_use]
    pub fn with_entitlement(mut self, gate: EntitlementGate) -> Self {
        self.entitlement = Arc::new(gate);
        self
    }

    /// Turn the failure bench and the user-route guard on or off (`INFERENCE_PROVIDER_BENCHING`).
    /// Off leaves every walk as it is today.
    #[must_use]
    pub fn with_benching(mut self, on: bool) -> Self {
        self.bench = on.then(|| Arc::new(super::health::BenchBoard::default()));
        self
    }

    /// Turn the same-tier fallback anchor on or off (`INFERENCE_SAME_TIER_FALLBACK`). Off leaves every
    /// walk as it is today.
    #[must_use]
    pub fn with_same_tier_fallback(mut self, on: bool) -> Self {
        self.same_tier = on;
        self
    }

    /// The route rules that run after every other gate of a chat walk: the user-route guard, then the
    /// same-tier anchor. `true` means the provider is skipped. A skipped-by-guard provider is not
    /// eligible, so it neither sets the anchor nor counts as an attempt. Logs name the provider and the
    /// tier labels, never content.
    fn route_rules_skip(
        &self,
        req: &InferRequest,
        name: &str,
        caps: &ProviderCapabilities,
        throttle: &mut ThrottleState,
    ) -> bool {
        if self.bench.is_some()
            && (caps.provider_id == mp_contracts::agentic_subscription::PROVIDER_ID
                || name == mp_contracts::agentic_subscription::PROVIDER_ID)
        {
            // A platform call never moves onto the user's subscription route (ownership-matrix rule 17):
            // only a request that names it and carries a connection may reach it.
            let named = !req.provider_hint.trim().is_empty()
                && Self::hint_matches(&req.provider_hint, name, &caps.aliases);
            if !(named && !req.subscription_connection_id.trim().is_empty()) {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    "provider skipped: a platform call is never moved onto the user subscription route"
                );
                return true;
            }
        }
        if self.same_tier {
            let tier = PrivacyTier::classify(caps);
            match throttle.anchor_tier {
                None => throttle.anchor_tier = Some(tier),
                Some(anchor) if tier < anchor => {
                    warn!(
                        provider = %name,
                        request_id = %req.request_id,
                        anchor_tier = anchor.label(),
                        provider_tier = tier.label(),
                        "provider skipped: privacy tier below the first eligible provider of this call"
                    );
                    return true;
                }
                Some(_) => {}
            }
        }
        false
    }

    /// Whether a provider is the user's subscription route. It is never benched and never records an
    /// outcome on the board.
    fn is_subscription_route(name: &str, caps: &ProviderCapabilities) -> bool {
        caps.provider_id == mp_contracts::agentic_subscription::PROVIDER_ID
            || name == mp_contracts::agentic_subscription::PROVIDER_ID
    }

    /// Whether a benched platform provider is skipped for `model` (`INFERENCE_PROVIDER_BENCHING`). Runs
    /// after every other gate and the route rules, so a provider counted here was otherwise eligible. The
    /// earliest bench end is kept in `throttle`. Logs name the provider and model, never content.
    fn bench_skip(
        &self,
        req: &InferRequest,
        name: &str,
        model: &str,
        caps: &ProviderCapabilities,
        throttle: &mut ThrottleState,
    ) -> bool {
        let Some(bench) = &self.bench else {
            return false;
        };
        if Self::is_subscription_route(name, caps) {
            return false;
        }
        let Some(until) = bench.benched_until(name, model, tokio::time::Instant::now()) else {
            return false;
        };
        throttle.note_benched(until);
        warn!(
            provider = %name,
            model = %model,
            request_id = %req.request_id,
            "provider skipped: benched after repeated failures"
        );
        true
    }

    /// Records one attempt outcome of a platform provider on the bench board
    /// (`INFERENCE_PROVIDER_BENCHING`). A success clears the pair's run; a classified failure is counted
    /// and may start a bench. Returns `true` when this failure started a bench, so the caller stops
    /// retrying the provider. A subscription route, and a subscription outcome from any provider, records
    /// nothing. Logs and metrics carry the provider, model and class only.
    fn bench_record(
        &self,
        req: &InferRequest,
        name: &str,
        model: &str,
        caps: &ProviderCapabilities,
        failure: Option<&ProviderError>,
    ) -> bool {
        let Some(bench) = &self.bench else {
            return false;
        };
        if Self::is_subscription_route(name, caps) {
            return false;
        }
        let now = tokio::time::Instant::now();
        let Some(error) = failure else {
            bench.record_success(name, model);
            return false;
        };
        if matches!(
            error,
            ProviderError::SubscriptionRefused { .. } | ProviderError::SubscriptionFailed(_)
        ) {
            return false;
        }
        let Some(class) = super::health::classify(error) else {
            return false;
        };
        crate::metrics::record_provider_failure(name, class.as_str());
        let retry_after = match error {
            ProviderError::RateLimited { retry_after_ms } if *retry_after_ms > 0 => {
                Some(Duration::from_millis(*retry_after_ms))
            }
            _ => None,
        };
        let Some(until) = bench.record_failure(name, model, &req.org_id, class, retry_after, now)
        else {
            return false;
        };
        crate::metrics::record_provider_benched(name, class.as_str());
        warn!(
            provider = %name,
            model = %model,
            class = class.as_str(),
            bench_ms = until.saturating_duration_since(now).as_millis(),
            "provider benched after repeated failures"
        );
        true
    }

    /// The `providers_benched` answer for a walk that made no attempt because every provider that
    /// passed the gates was benched: the wait until the earliest bench ends, at least 1 ms.
    fn providers_benched_error(until: tokio::time::Instant) -> ProviderError {
        let wait = until.saturating_duration_since(tokio::time::Instant::now());
        let retry_after_ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX).max(1);
        ProviderError::ProvidersBenched { retry_after_ms }
    }

    /// The gate the handlers resolve a caller's entitlement through.
    #[must_use]
    pub fn entitlement(&self) -> &EntitlementGate {
        &self.entitlement
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
    /// to a concrete model, returning a rewritten request and, while the route
    /// note is on, what the budget did to it (WS27-S7). Returns `None` when the
    /// intent layer is off or the model is a pinned id (pass through).
    ///
    /// The pick is judged against both catalogue overlays here, as the walk judges every model it tries: a pick
    /// either one hides gives way to the next visible candidate (see [`Self::overlay_substitute`]), and the route
    /// note then describes the model served, not the pick (V3-inf-1).
    async fn resolve_intent(
        &self,
        req: &InferRequest,
        org: Option<&OrgOverlay>,
    ) -> Option<(InferRequest, Option<RouteNote>)> {
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
        let substitute = self.overlay_substitute(req, org, &policy, &decision.model);
        if let Some(served) = &substitute {
            info!(
                request_id = %req.request_id,
                resolved_model = %decision.model,
                served_model = %served,
                "intent pick hidden by a catalogue overlay; served by the next visible candidate"
            );
        }
        let served = substitute.as_ref().unwrap_or(&decision.model);
        let mut rewritten = req.clone();
        rewritten.model.clone_from(served);
        // What the budget cut, for the route note: the effort it clamped to, when a clamp fired.
        let mut applied_effort = None;
        if effort::budget_clamp_enabled() {
            if let Some(requested) = rewritten.reasoning_effort {
                let applied = effort::clamp_to_cap(requested, decision.effort_cap);
                if applied != requested {
                    // Visible on purpose: the user asked for more than they got.
                    info!(
                        request_id = %req.request_id,
                        resolved_model = %rewritten.model,
                        posture = ?decision.posture,
                        requested = requested.as_str(),
                        applied = applied.as_str(),
                        clamped = true,
                        "effort clamped by budget posture"
                    );
                    rewritten.reasoning_effort = Some(applied);
                    applied_effort = Some(applied);
                }
            } else {
                // A request with no level carries only the legacy budget, which is
                // what the gateway sends when its effort ladder is off. Without this
                // branch an exhausted org would still spend the whole budget.
                let budget = rewritten.thinking_budget_tokens;
                let applied = effort::cap_legacy_budget(budget, decision.effort_cap);
                if applied != budget {
                    info!(
                        request_id = %req.request_id,
                        resolved_model = %rewritten.model,
                        posture = ?decision.posture,
                        requested_budget = budget,
                        applied_budget = applied,
                        clamped = true,
                        "thinking budget clamped by budget posture"
                    );
                    rewritten.thinking_budget_tokens = applied;
                    applied_effort = decision.effort_cap;
                }
            }
        }
        let moved = substitute.is_some();
        let note = route_note_enabled().then(|| {
            if moved {
                overlay_route_note(&decision, applied_effort)
            } else {
                budget_route_note(&policy, &decision, applied_effort)
            }
        });
        Some((rewritten, note))
    }

    /// The request a `verevon-role` call runs as: its model replaced by the model the role table chose,
    /// with the decision for the route note. `None` for every other model, which is never touched here.
    ///
    /// # Errors
    ///
    /// [`ProviderError::RequestRejected`] when the call names no purpose the role table serves. Nothing
    /// has been sent anywhere.
    fn resolve_role_call(
        &self,
        req: &InferRequest,
        role: Option<&RoleCall>,
        org: Option<&OrgOverlay>,
    ) -> Result<Option<(InferRequest, role::RoleDecision)>, ProviderError> {
        if !req.model.trim().eq_ignore_ascii_case(role::ROLE_MODEL_ID) {
            return Ok(None);
        }
        let purpose = role
            .and_then(|call| call.purpose)
            .ok_or_else(|| ProviderError::RequestRejected {
                detail: format!(
                    "`{}` needs a side-task purpose that the role table serves",
                    role::ROLE_MODEL_ID
                ),
            })?;
        let fallback_model = role.map_or("", |call| call.fallback_model.as_str());
        let catalogue = self.list_models("chat", &req.provider_hint);
        // The caller's organisation's own candidates for the purpose, tried before the platform table's
        // (INFERENCE_ORG_OVERLAY).
        let org_candidates = self.org_role_candidates(org, purpose, &catalogue);
        let policy = self.policy.load();
        let choice =
            role::choose_with_org(&policy, &org_candidates, purpose, fallback_model, |model| {
                self.role_candidate_admitted(req, &catalogue, org, model)
            });
        let mut resolved = req.clone();
        resolved.model.clone_from(&choice.model);
        // The table's effort is the role's default, so it applies only when the caller names no effort.
        if let Some(level) = choice.effort {
            if resolved.reasoning_effort.is_none() && resolved.thinking_budget_tokens == 0 {
                resolved.reasoning_effort = Some(level);
            }
        }
        info!(
            request_id = %req.request_id,
            role = purpose.as_str(),
            role_source = choice.decision.source.as_str(),
            resolved_model = %choice.model,
            "verevon role resolved"
        );
        Ok(Some((resolved, choice.decision)))
    }

    /// The organisation's role candidates for `purpose` (W4-BYO-3): the ids its overlay names that the platform
    /// catalogue lists, minus any the platform overlay hides. Empty when the call has no organisation overlay (`org`
    /// is `None`, see [`Self::org_overlay_for`]). Each candidate is still asked of [`Self::role_candidate_admitted`],
    /// so the entitlement, ZDR, privacy-tier and residency floors apply to it.
    fn org_role_candidates(
        &self,
        org: Option<&OrgOverlay>,
        purpose: RolePurpose,
        catalogue: &[ModelInfo],
    ) -> Vec<String> {
        let Some(overlay) = org else {
            return Vec::new();
        };
        let mut candidates = overlay.role_candidates(purpose, catalogue);
        if catalogue_overlay::catalogue_overlay_enabled() {
            let policy = self.policy.load();
            candidates.retain(|id| !policy.catalogue_overlay.is_hidden(id));
        }
        candidates
    }

    /// Whether a role candidate may serve this call (EP-13 S4). A candidate the chain does not list is
    /// refused. A listed one must be served by a provider the call's hint allows, and that provider must
    /// clear the call's ZDR, privacy-tier, residency, image and entitlement gates, the gates
    /// `infer_one_model` applies, so the walk never has to refuse a candidate the table chose. An intent
    /// id is admitted, because the intent layer resolves it and the walk gates the result. A model the platform
    /// overlay or the caller's organisation overlay hides is refused, as a named one is refused at admission, so the
    /// next candidate is chosen.
    fn role_candidate_admitted(
        &self,
        req: &InferRequest,
        catalogue: &[ModelInfo],
        org: Option<&OrgOverlay>,
        model: &str,
    ) -> bool {
        if intent::parse_mode(model).is_some() {
            return true;
        }
        if self.platform_hides(model) || org_hides(org, model) {
            return false;
        }
        if !catalogue.iter().any(|known| known.id.eq_ignore_ascii_case(model)) {
            return false;
        }
        self.providers.iter().any(|(name, provider)| {
            let caps = provider.capabilities_dyn();
            Self::hint_matches(&req.provider_hint, name, &caps.aliases)
                && provider_serves_model(&caps, self.catalog_for(name), model)
                && (!req.zdr || caps.supports_zdr)
                && provider_meets_privacy_tier(&caps, req.min_privacy_tier)
                && !residency_gate_blocks(req, caps.residency)
                && req
                    .access
                    .as_ref()
                    .is_none_or(|access| access.refuses(model, name).is_none())
                && (req.image_parts.is_empty() || model_accepts_images(&caps, model))
        })
    }

    /// Lower a request's effort to the org's `max_effort` ceiling. model-gateway
    /// clamps the same ceiling for the callers it fronts; this is the enforcement
    /// point for a caller that reaches inference-core directly, such as the agent
    /// loop, which would otherwise run a deep loop under a role capped at `low`.
    /// It clamps down only, and only when the policy is enforced: shadow mode
    /// serves the request as it is. A Claude 5.5 request that names neither a
    /// level nor a budget runs at its model's default, so that default is what the
    /// ceiling bounds. Returns `None` when nothing changes, so the common path
    /// neither clones the request nor reads the policy.
    ///
    /// It runs before the response cache is read, because the cache key carries
    /// the effort level: an answer made above the ceiling must not answer a
    /// request the ceiling now bounds.
    fn apply_effort_ceiling(req: &InferRequest) -> Option<InferRequest> {
        let access = req.access.as_deref().filter(|access| access.enforced())?;
        // An empty or unknown ceiling limits nothing: org-core validates the name
        // when the policy is written, and a stray value must not lower a request.
        let ceiling = effort::Level::from_name(&access.resolved().policy.as_ref()?.max_effort)?;
        // A Claude 5.5 request that names neither a level nor a budget runs at its
        // model's default level, which the adapter sends explicitly, so the ceiling
        // bounds that level as if it had been requested. The budget test is `<= 0`
        // because that is when the adapter falls back to the default: a positive
        // budget is read as a level by `from_legacy_budget`.
        let model_default = if req.reasoning_effort.is_none() && req.thinking_budget_tokens <= 0 {
            super::anthropic::claude_5_5_default_effort(&req.model)
        } else {
            None
        };
        let reasoning_effort = match (req.reasoning_effort, model_default) {
            (Some(requested), _) => Some(effort::clamp_to_cap(requested, Some(ceiling))),
            // Set only when the ceiling lowers the default: a default the ceiling
            // leaves alone stays unset, so the request reaches the adapter as it was.
            (None, Some(default)) => {
                let bounded = effort::clamp_to_cap(default, Some(ceiling));
                (bounded < default).then_some(bounded)
            }
            (None, None) => None,
        };
        // A request with no level is bounded by its legacy budget instead.
        let thinking_budget_tokens = if req.reasoning_effort.is_none() {
            effort::cap_legacy_budget(req.thinking_budget_tokens, Some(ceiling))
        } else {
            req.thinking_budget_tokens
        };
        if reasoning_effort == req.reasoning_effort
            && thinking_budget_tokens == req.thinking_budget_tokens
        {
            return None;
        }
        info!(
            request_id = %req.request_id,
            ceiling = ceiling.as_str(),
            clamped = true,
            "effort clamped by the org's effort ceiling"
        );
        let mut clamped = req.clone();
        clamped.reasoning_effort = reasoning_effort;
        clamped.thinking_budget_tokens = thinking_budget_tokens;
        Some(clamped)
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
            "openai-codex-subscription" => &self.defaults.codex_subscription,
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
    /// [`MAX_TOOL_LADDER_STEPS`] and pruned of rungs this resource cannot serve or the platform overlay hides.
    ///
    /// Callers must only pass a model the intent layer resolved from a
    /// `verevon-*` mode. Substituting under a caller who pinned a model would
    /// answer with something they did not ask for; "the model I chose was busy"
    /// is their decision to make, not ours to paper over.
    ///
    /// For a caller whose organisation restricts models, a rung the policy
    /// refuses is dropped before the step bound is counted, so a denied rung
    /// never uses up one of the [`MAX_TOOL_LADDER_STEPS`] a permitted one could
    /// have had. A rung the platform or the caller's organisation overlay hides
    /// is dropped the same way.
    fn tool_ladder(
        &self,
        resolved: &str,
        access: Option<&RequestAccess>,
        hint: &str,
        org: Option<&OrgOverlay>,
    ) -> Vec<String> {
        let policy = self.policy.load();
        self.visible_candidates(&policy.tool_fallback_ladder, resolved, access, hint, org)
    }

    /// The candidates the walk may try after `resolved`, in their order: each one that is not `resolved`, is not
    /// already listed, is deployed, is hidden by neither catalogue overlay and is not refused by the caller's
    /// entitlement, bounded by [`MAX_TOOL_LADDER_STEPS`]. A candidate an overlay hides is dropped before the bound is
    /// counted.
    fn visible_candidates(
        &self,
        candidates: &[String],
        resolved: &str,
        access: Option<&RequestAccess>,
        hint: &str,
        org: Option<&OrgOverlay>,
    ) -> Vec<String> {
        let mut ladder: Vec<String> = Vec::new();
        for candidate in candidates {
            if ladder.len() >= MAX_TOOL_LADDER_STEPS {
                break;
            }
            let already_queued = candidate.eq_ignore_ascii_case(resolved)
                || ladder.iter().any(|m| m.eq_ignore_ascii_case(candidate));
            if already_queued
                || !self.is_deployed(candidate)
                || self.platform_hides(candidate)
                || org_hides(org, candidate)
            {
                continue;
            }
            if access
                .is_some_and(|access| self.entitlement_refusal(access, candidate, hint).is_some())
            {
                continue;
            }
            ladder.push(candidate.clone());
        }
        ladder
    }

    /// The model an intent pick is served as when a catalogue overlay hides it: the first candidate the walk would try
    /// after the pick that no overlay hides. The candidates are the tool ladder's rungs in their order, then the
    /// policy's cheap fallback, each passing the checks the ladder makes. `None` when the pick is not hidden, and when
    /// no candidate is visible: the pick then stays, and the walk refuses it as `model_hidden`, as before.
    fn overlay_substitute(
        &self,
        req: &InferRequest,
        org: Option<&OrgOverlay>,
        policy: &RoutingPolicy,
        pick: &str,
    ) -> Option<String> {
        if !(self.platform_hides(pick) || org_hides(org, pick)) {
            return None;
        }
        let mut candidates = policy.tool_fallback_ladder.clone();
        candidates.push(policy.cheap_fallback.clone());
        self.visible_candidates(
            &candidates,
            pick,
            req.access.as_deref(),
            &req.provider_hint,
            org,
        )
        .into_iter()
        .next()
    }

    /// Why the caller's entitlement refuses `model` on every provider that could
    /// serve it, or `None` when some provider may, when no provider matches (the
    /// walk then ends in its usual way) or when `model` is unspecified (each
    /// provider's own default is judged where it is chosen).
    ///
    /// The provider is part of the question because a policy may name a whole
    /// provider (`provider:openai`), and a model id alone does not say which
    /// provider would serve it.
    fn entitlement_refusal(&self, access: &RequestAccess, model: &str, hint: &str) -> Option<Reason> {
        if is_unspecified_model(model) {
            return None;
        }
        let mut first = None;
        for (name, provider) in &self.providers {
            let caps = provider.capabilities_dyn();
            if !Self::hint_matches(hint, name, &caps.aliases)
                || !provider_serves_model(&caps, self.catalog_for(name), model)
            {
                continue;
            }
            match access.refuses(model, name) {
                None => return None,
                Some(reason) => {
                    first.get_or_insert(reason);
                }
            }
        }
        first
    }

    /// Whether the platform catalogue overlay hides the model a call would be served as, while
    /// `INFERENCE_CATALOGUE_OVERLAY` is on. Off, nothing is hidden and every route is as it was. A retired
    /// Claude id is served as its successor, so the successor is checked too. The routes the caller does not
    /// name (the intent layer, the role table, the tool ladder) pick a model with no admission check of their
    /// own, so each consults this where it picks. A walk consults it before any provider is asked, on the model
    /// that provider would really serve: its default for an automatic call, which names no model.
    fn platform_hides(&self, model: &str) -> bool {
        if !catalogue_overlay::catalogue_overlay_enabled() {
            return false;
        }
        let policy = self.policy.load();
        let overlay = &policy.catalogue_overlay;
        overlay.is_hidden(model) || overlay.is_hidden(served_model(model))
    }

    /// The caller's organisation overlay for one call (W4-BYO-3), read once at the top of the call so that the
    /// admission, the role table, the tool ladder, the cache and every rung of the walk judge the same overlay.
    /// `None` while `INFERENCE_ORG_OVERLAY` is off, when no policy store is wired or the organisation id is blank, so
    /// nothing below changes. The client caches each organisation's overlay, so the read is cheap after the first.
    async fn org_overlay_for(&self, org_id: &str) -> Option<Arc<OrgOverlay>> {
        if !catalogue_overlay::org_overlay_enabled() || org_id.trim().is_empty() {
            return None;
        }
        let client = self.policy_client.as_ref()?;
        Some(client.org_overlay(org_id).await)
    }

    /// The overlay that refuses a call for `model` for this organisation, judged as admission judges it: an alias by
    /// its target, and a retired Claude id by its successor. `None` when no overlay hides it, and for a model that
    /// names no concrete id (an automatic, `verevon-*` or role id), which the routing layer answers for itself. Nothing
    /// is sent. The access answer reads it, so it never reports as available a model that infer would refuse.
    pub async fn hiding_overlay(&self, org_id: &str, model: &str) -> Option<HidingOverlay> {
        if catalogue_overlay::is_reserved_id(model) {
            return None;
        }
        let mut resolved = model.to_owned();
        if catalogue_overlay::catalogue_overlay_enabled() {
            let policy = self.policy.load();
            let overlay = &policy.catalogue_overlay;
            if !overlay.is_empty() {
                let target = overlay.alias_target(model).map(str::to_owned);
                let named = target.as_deref().unwrap_or(model);
                if overlay.is_hidden(named) || overlay.is_hidden(served_model(named)) {
                    return Some(HidingOverlay::Platform);
                }
                if let Some(target) = target {
                    resolved = target;
                }
            }
        }
        let org = self.org_overlay_for(org_id).await;
        org_hides(org.as_deref(), &resolved).then_some(HidingOverlay::Organisation)
    }

    /// The catalogue overlay at admission, while `INFERENCE_CATALOGUE_OVERLAY` is on.
    /// An alias is answered as the model it stands for, and a hidden model is refused
    /// before any provider is called or the entitlement check names it. `None` leaves
    /// the request as it was: the overlay is off or empty, or the request names no
    /// concrete model (an automatic, `verevon-*` or role id), which the routing layer
    /// answers for itself.
    ///
    /// # Errors
    ///
    /// [`ProviderError::ModelHidden`] when the model the caller named, the model its
    /// alias stands for, or the model that serves it (the successor of a retired Claude
    /// id) is hidden by the platform overlay, or (while `INFERENCE_ORG_OVERLAY` is on) by
    /// the caller's own organisation overlay. Nothing has been sent anywhere.
    fn overlay_admission(
        &self,
        req: &InferRequest,
        org: Option<&OrgOverlay>,
    ) -> Result<Option<InferRequest>, ProviderError> {
        if catalogue_overlay::is_reserved_id(&req.model) {
            return Ok(None);
        }
        // The platform overlay.
        let mut mapped: Option<InferRequest> = None;
        if catalogue_overlay::catalogue_overlay_enabled() {
            let policy = self.policy.load();
            let overlay = &policy.catalogue_overlay;
            if !overlay.is_empty() {
                let target = overlay.alias_target(&req.model);
                let named = target.unwrap_or(req.model.as_str());
                // A retired Claude id is served as its successor, so the successor is checked
                // too: the check is by the id that would really be served.
                if overlay.is_hidden(named) || overlay.is_hidden(served_model(named)) {
                    return Err(ProviderError::ModelHidden {
                        model: req.model.clone(),
                    });
                }
                if let Some(target) = target {
                    info!(
                        request_id = %req.request_id,
                        alias = %req.model,
                        resolved_model = %target,
                        "catalogue alias resolved"
                    );
                    let mut aliased = req.clone();
                    aliased.model = target.to_owned();
                    mapped = Some(aliased);
                }
            }
        }
        // The caller's organisation overlay (W4-BYO-3), by the resolved id, so an alias that stands for a model the
        // organisation hides is refused too. As on the platform overlay, the model served is checked as well: a
        // retired id is answered by its successor.
        let resolved = mapped
            .as_ref()
            .map_or(req.model.as_str(), |m| m.model.as_str());
        if org_hides(org, resolved) {
            info!(
                request_id = %req.request_id,
                "model refused: hidden by the organisation overlay"
            );
            return Err(ProviderError::ModelHidden {
                model: req.model.clone(),
            });
        }
        Ok(mapped)
    }

    /// The model the caller named, after checking it against their entitlement:
    /// `None` for an unspecified (automatic) model, `Some` for a pinned one the
    /// policy allows on some provider.
    ///
    /// # Errors
    /// [`ProviderError::ModelNotEntitled`] when the policy refuses a pinned model
    /// on every provider that could serve it. Nothing has been sent anywhere.
    fn pinned_model<'a>(&self, req: &'a InferRequest) -> Result<Option<&'a str>, ProviderError> {
        if is_unspecified_model(&req.model) {
            return Ok(None);
        }
        if let Some(access) = &req.access {
            if let Some(reason) = self.entitlement_refusal(access, &req.model, &req.provider_hint) {
                return Err(access.refusal(&req.model, reason));
            }
        }
        Ok(Some(&req.model))
    }

    /// Whether a stored response may be served to this caller: it was made by a
    /// model their organisation still allows. The cache key does not carry the
    /// policy, so a revocation must be checked here or an old answer outlives it.
    fn entitled_hit(&self, req: &InferRequest, model_used: &str, provider_used: &str) -> bool {
        req.access
            .as_ref()
            .is_none_or(|access| access.refuses(model_used, provider_used).is_none())
    }

    /// The error that ends a walk in which no candidate was tried because the
    /// caller's entitlement skipped each one. `pinned` is the model the caller
    /// named, if they named one; an automatic request names no model.
    fn entitlement_exhaustion(
        req: &InferRequest,
        throttle: &ThrottleState,
        total_attempts: u32,
        pinned: Option<&str>,
    ) -> Option<ProviderError> {
        if total_attempts != 0 {
            return None;
        }
        let access = req.access.as_ref()?;
        let denial = throttle.entitlement_denial?;
        Some(match pinned {
            Some(model) => access.refusal(model, denial),
            None => access.refusal("", Reason::NoEntitledRoute),
        })
    }

    /// Perform unary inference with fallback and caching.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every provider and retry is exhausted.
    pub async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        self.infer_noted(req, true)
            .await
            .map(|(response, _)| response)
    }

    /// Fresh inference for checks that must not reuse or store a completed
    /// response. Provider prefix caching and all route/privacy gates remain.
    pub async fn infer_without_response_cache(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        self.infer_noted(req, false)
            .await
            .map(|(response, _)| response)
    }

    /// The unary call with the route note the intent layer stamped on it (WS27-S7), when the note is on.
    /// The note is never stored with a response, so a cache hit carries the note of the call that asked.
    pub async fn infer_noted(
        &self,
        req: &InferRequest,
        cache_response: bool,
    ) -> Result<(InferResponse, Option<RouteNote>), ProviderError> {
        self.infer_noted_with_role(req, None, cache_response).await
    }

    /// [`Self::infer_noted`] for a `verevon-role` call (EP-13 S4): the role table picks the model, and
    /// the chosen model runs as an ordinary call with the decision recorded on its route note.
    ///
    /// # Errors
    ///
    /// [`ProviderError::RequestRejected`] when the call names no purpose the role table serves, and
    /// every error [`Self::infer_noted`] can return for the chosen model.
    pub async fn infer_role_noted(
        &self,
        req: &InferRequest,
        role: &RoleCall,
        cache_response: bool,
    ) -> Result<(InferResponse, Option<RouteNote>), ProviderError> {
        self.infer_noted_with_role(req, Some(role), cache_response).await
    }

    async fn infer_noted_with_role(
        &self,
        req: &InferRequest,
        role: Option<&RoleCall>,
        cache_response: bool,
    ) -> Result<(InferResponse, Option<RouteNote>), ProviderError> {
        let mut clock = CallClock::start();
        // The id the caller sent, before the role table and the intent layer resolve it: a walk that refuses
        // for a hidden model names it.
        let named = req.model.as_str();
        // The caller's organisation overlay, read once for the whole call, so the admission, the role table, the
        // tool ladder, the cache and every rung of the walk judge the same overlay (W4-BYO-3).
        let org_overlay = self.org_overlay_for(&req.org_id).await;
        let org = org_overlay.as_deref();
        // The catalogue overlay answers an alias as its target and refuses a hidden
        // model, before the retired-id, role and entitlement checks name it.
        let aliased = self.overlay_admission(req, org)?;
        let req: &InferRequest = aliased.as_ref().unwrap_or(req);
        // A retired Claude id is answered by its successor, so the entitlement check,
        // the cache key and every rung of the ladder all name the model that serves.
        let current = super::anthropic::with_current_model(req);
        let req: &InferRequest = current.as_ref().unwrap_or(req);
        // A `verevon-role` call is resolved to a concrete model before anything else reads it, so the
        // name never reaches the entitlement check, the intent layer or a provider (EP-13 S4).
        let roled = self.resolve_role_call(req, role, org)?;
        let role_decision = roled.as_ref().map(|(_, decision)| *decision);
        let req: &InferRequest = roled.as_ref().map_or(req, |(resolved, _)| resolved);
        // A model the caller named is checked against their organisation's
        // entitlement first of all: before the intent layer spends a budget call
        // and before the cache can answer with a model they may not use.
        let pinned = self.pinned_model(req)?;
        // Verevon intent layer: resolve a `verevon-*` mode to a concrete model
        // (complexity + budget) before anything else. A pinned model or a
        // disabled intent layer leaves `req` untouched.
        let (intent_req, route_note) = self
            .resolve_intent(req, org)
            .await
            .map_or((None, None), |(resolved, note)| (Some(resolved), note));
        let route_note = attach_role(route_note, role_decision);
        // The ladder exists only for a model WE chose. `intent_req.is_some()` is
        // exactly that condition — see `attempt_models`.
        let ladder = intent_req.as_ref().map_or_else(Vec::new, |resolved| {
            self.tool_ladder(
                &resolved.model,
                resolved.access.as_deref(),
                &resolved.provider_hint,
                org,
            )
        });
        let req: &InferRequest = intent_req.as_ref().unwrap_or(req);
        // The org's effort ceiling bounds the request before the cache is read,
        // because the cache key carries the effort level.
        let ceiled = Self::apply_effort_ceiling(req);
        let req: &InferRequest = ceiled.as_ref().unwrap_or(req);
        validate_min_residency(req)?;
        // An image is part of the question but not of the cache key, and a
        // stored answer about one image must never answer another: a request
        // with images neither reads nor fills the response cache.
        // A model the platform or the organisation overlay hides is neither answered from the cache nor stored in
        // it: the walk refuses it, and a stored answer must not outlive the hiding.
        let cache_response = cache_response
            && req.image_parts.is_empty()
            && !self.platform_hides(&req.model)
            && !org_hides(org, &req.model);

        // Check cache first
        if cache_response {
            if let Some(mut cached) = self.cache.get(req).filter(|cached| {
                // The key names the model the call asked for and does not carry the overlays, so a stored answer
                // from a model the platform or the organisation overlay now hides is not served (as a revoked
                // entitlement is not).
                self.entitled_hit(req, &cached.model_used, &cached.provider_used)
                    && !self.platform_hides(&cached.model_used)
                    && !org_hides(org, &cached.model_used)
            }) {
                info!(request_id = %req.request_id, "cache hit");
                // The stored copy may carry the timing of the call that filled
                // the cache; this call reached no provider at all.
                cached.provider_timing = Some(clock.cache_hit(&cached));
                return Ok((cached, route_note));
            }
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
                let wait_began = tokio::time::Instant::now();
                tokio::time::sleep(wait).await;
                clock.waited(wait_began);
                throttle.next_pass();
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
                    .infer_one_model(
                        req,
                        model,
                        &mut total_attempts,
                        &mut throttle,
                        &mut clock,
                        org,
                    )
                    .await
                {
                    if cache_response { self.cache.put(req, &response); }
                    return Ok((response, route_note));
                }
                // A refused subscription is not answered by the next rung.
                if throttle.refused() {
                    break;
                }
            }

            if throttle.refused() || !throttle.throttled() {
                // Nothing was throttled, so waiting cannot help; and a
                // subscription refusal is never waited out here.
                break;
            }
        }

        let benched = throttle
            .benched_until
            .filter(|_| self.bench.is_some() && total_attempts == 0);
        if let Some(until) = benched {
            // Every provider that passed the gates was benched: retry later, not a terminal posture
            // refusal.
            Err(Self::providers_benched_error(until))
        } else if let Some(refusal) = Self::entitlement_exhaustion(req, &throttle, total_attempts, pinned) {
            Err(refusal)
        } else if throttle.hidden_skipped && total_attempts == 0 {
            Err(ProviderError::ModelHidden {
                model: named.to_owned(),
            })
        } else if throttle.vision_skipped && total_attempts == 0 {
            Err(vision_unsupported_error())
        } else if req.min_privacy_tier > PrivacyTier::Unspecified && total_attempts == 0 {
            Err(tier_unavailable_error(req.min_privacy_tier))
        } else if req.zdr && total_attempts == 0 {
            Err(ProviderError::ZdrUnavailable(
                "no matching provider deployment has verified ZDR support".to_owned(),
            ))
        } else if !req.min_residency.trim().is_empty() && total_attempts == 0 {
            Err(ProviderError::ResidencyViolation(format!(
                "no matching provider deployment meets the requested `{}` residency floor",
                req.min_residency.trim()
            )))
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
        clock: &mut CallClock,
        org: Option<&OrgOverlay>,
    ) -> Option<InferResponse> {
        // The platform overlay or the caller's organisation overlay hides this model: nothing is sent and the walk
        // moves on. A named model was refused at admission, so this is a model the intent layer, the role table, the
        // ladder or an automatic default chose.
        if self.platform_hides(model) || org_hides(org, model) {
            throttle.hidden_skipped = true;
            return None;
        }
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
            // Residency-floor gate: the tier gate above answers "is this
            // deployment's posture strong enough"; this one answers the
            // caller's explicit `min_residency` token. Both run before any
            // network call, and a provider must clear BOTH to be tried.
            if residency_gate_blocks(req, caps.residency) {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    required = %req.min_residency,
                    declared = caps.residency.as_str(),
                    "provider skipped: residency floor not met"
                );
                continue;
            }
            let call_req = self.request_for(req, model, name);
            // The model this provider would really serve is checked against the overlays before anything is sent. For
            // an automatic call that is the provider default, which the top-of-walk check cannot see.
            if self.platform_hides(&call_req.model) || org_hides(org, &call_req.model) {
                throttle.hidden_skipped = true;
                continue;
            }
            // The caller's entitlement, judged on the model this provider would
            // really serve (its own default for an unspecified one).
            if let Some(reason) = req
                .access
                .as_ref()
                .and_then(|access| access.refuses(&call_req.model, name))
            {
                throttle.entitlement_denial.get_or_insert(reason);
                continue;
            }
            // Vision gate: a request that carries images is never sent to a
            // provider or model that cannot read them, and never answered from
            // its text alone. Skipped before any network call, like the gates
            // above; the exhaustion below names it.
            if !req.image_parts.is_empty() && !model_accepts_images(&caps, &call_req.model) {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    "provider skipped: the request carries images and this model does not accept them"
                );
                throttle.vision_skipped = true;
                continue;
            }
            if self.route_rules_skip(req, name, &caps, throttle) {
                continue;
            }
            if self.bench_skip(req, name, &call_req.model, &caps, throttle) {
                continue;
            }
            for attempt in 1..=self.max_retries {
                *total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                );
                let began = clock.begin_attempt();
                // The call is instrumented rather than run under `span.enter()`:
                // an entered guard held across the await stays on this worker
                // thread while the task is parked, so whatever else ran there
                // was logged under this request, and this call's own logs lost
                // it once the task resumed on another thread. The span is
                // entered again only for the synchronous outcome logs below.
                let outcome = provider
                    .infer_dyn(call_req.as_ref())
                    .instrument(span.clone())
                    .await;
                if outcome.is_err() {
                    clock.attempt_failed(began);
                }
                let _enter = span.enter();
                let benched_now = self.bench_record(
                    req,
                    name,
                    &call_req.model,
                    &caps,
                    outcome.as_ref().err(),
                );

                match outcome {
                    Ok(mut response) => {
                        let timing = clock.served(began, *total_attempts, &mut response);
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
                        response.provider_timing = Some(timing);
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
                    Err(ProviderError::TooLong { detail }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            "prompt too long for this provider; walking on for a larger window"
                        );
                        throttle.record_too_long(&detail);
                        break; // Deterministic for this provider — retrying cannot help.
                    }
                    Err(ProviderError::RequestRejected { detail }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            "provider rejected the request as malformed; walking on"
                        );
                        throttle.record_rejected(&detail);
                        break; // Deterministic for this provider — the same request is refused again.
                    }
                    Err(ProviderError::SubscriptionRefused {
                        reason,
                        retry_after_ms,
                        resets_at,
                    }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            reason = reason.code(),
                            resets_at = %reset_label(resets_at),
                            "subscription refused; not retrying"
                        );
                        throttle.record_refusal(ProviderError::SubscriptionRefused {
                            reason,
                            retry_after_ms,
                            resets_at,
                        });
                        // One attempt, and no other provider or model may
                        // answer for this account: the walk ends here.
                        return None;
                    }
                    Err(ProviderError::SubscriptionFailed(message)) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            error = %message,
                            "subscription turn failed after it started; not retrying"
                        );
                        throttle.record_refusal(ProviderError::SubscriptionFailed(message));
                        return None;
                    }
                    Err(e) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            error = %e,
                            "provider attempt failed"
                        );
                        if benched_now {
                            break; // This failure benched the provider: no more attempts on it.
                        }
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
        // The effort ceiling ran on the unresolved id (an unpinned "Verevon Auto"
        // request, or the pinned model before a ladder rung replaced it), where a
        // Claude 5.5 default is invisible. Bound the resolved model too: the clamp
        // only lowers and is idempotent, so an already bounded request is unchanged.
        if let Some(ceiled) = Self::apply_effort_ceiling(&rewritten) {
            rewritten = ceiled;
        }
        std::borrow::Cow::Owned(rewritten)
    }

    /// Fill serving-provider provenance on every chunk of a stream. Final
    /// chunks carry it verbatim; non-final chunks keep empty fields so callers
    /// reading only terminal frames still see the truth about who served the
    /// turn.
    ///
    /// This task sees every chunk, so it also finishes the stream's timing:
    /// the first non-empty content or reasoning delta marks the first token,
    /// and the final chunk closes the provider interval and carries it.
    fn stamp_stream_provenance(
        mut rx: mpsc::Receiver<InferChunk>,
        provider_used: String,
        residency: String,
        mut clock: StreamClock,
    ) -> mpsc::Receiver<InferChunk> {
        let (tx, out_rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(mut chunk) = rx.recv().await {
                if chunk.done || chunk.provider_used.is_empty() {
                    chunk.provider_used.clone_from(&provider_used);
                    chunk.residency.clone_from(&residency);
                }
                clock.observe(&mut chunk);
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
        self.infer_stream_noted(req).await.map(|(rx, _)| rx)
    }

    /// The stream with the route note the intent layer stamped on it (WS27-S7), when the note is on. The
    /// note rides the final chunk on the wire, so the caller attaches it there.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every provider and retry is exhausted.
    pub async fn infer_stream_noted(
        &self,
        req: &InferRequest,
    ) -> Result<(mpsc::Receiver<InferChunk>, Option<RouteNote>), ProviderError> {
        self.infer_stream_noted_with_role(req, None).await
    }

    /// [`Self::infer_stream_noted`] for a `verevon-role` call (EP-13 S4), resolved as on the unary path.
    ///
    /// # Errors
    ///
    /// The errors of [`Self::infer_role_noted`].
    pub async fn infer_stream_role_noted(
        &self,
        req: &InferRequest,
        role: &RoleCall,
    ) -> Result<(mpsc::Receiver<InferChunk>, Option<RouteNote>), ProviderError> {
        self.infer_stream_noted_with_role(req, Some(role)).await
    }

    async fn infer_stream_noted_with_role(
        &self,
        req: &InferRequest,
        role: Option<&RoleCall>,
    ) -> Result<(mpsc::Receiver<InferChunk>, Option<RouteNote>), ProviderError> {
        let mut clock = CallClock::start();
        // The id the caller sent, as on the unary path: a walk that refuses for a hidden model names it.
        let named = req.model.as_str();
        // The caller's organisation overlay, read once for the whole call, as on the unary path.
        let org_overlay = self.org_overlay_for(&req.org_id).await;
        let org = org_overlay.as_deref();
        // The catalogue overlay answers an alias as its target and refuses a hidden
        // model, as on the unary path.
        let aliased = self.overlay_admission(req, org)?;
        let req: &InferRequest = aliased.as_ref().unwrap_or(req);
        // A retired Claude id is answered by its successor, as on the unary path.
        let current = super::anthropic::with_current_model(req);
        let req: &InferRequest = current.as_ref().unwrap_or(req);
        // A `verevon-role` call is resolved first, as on the unary path.
        let roled = self.resolve_role_call(req, role, org)?;
        let role_decision = roled.as_ref().map(|(_, decision)| *decision);
        let req: &InferRequest = roled.as_ref().map_or(req, |(resolved, _)| resolved);
        // Entitlement of a pinned model first, as on the unary path.
        let pinned = self.pinned_model(req)?;
        // Verevon intent layer — same resolution as the unary path.
        let (intent_req, route_note) = self
            .resolve_intent(req, org)
            .await
            .map_or((None, None), |(resolved, note)| (Some(resolved), note));
        let route_note = attach_role(route_note, role_decision);
        let ladder = intent_req.as_ref().map_or_else(Vec::new, |resolved| {
            self.tool_ladder(
                &resolved.model,
                resolved.access.as_deref(),
                &resolved.provider_hint,
                org,
            )
        });
        let req: &InferRequest = intent_req.as_ref().unwrap_or(req);
        let ceiled = Self::apply_effort_ceiling(req);
        let req: &InferRequest = ceiled.as_ref().unwrap_or(req);
        validate_min_residency(req)?;

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
                let wait_began = tokio::time::Instant::now();
                tokio::time::sleep(wait).await;
                clock.waited(wait_began);
                throttle.next_pass();
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
                    .stream_one_model(
                        req,
                        model,
                        &mut total_attempts,
                        &mut throttle,
                        &mut clock,
                        org,
                    )
                    .await
                {
                    return Ok((rx, route_note));
                }
                if throttle.refused() {
                    break;
                }
            }

            if throttle.refused() || !throttle.throttled() {
                break;
            }
        }

        let benched = throttle
            .benched_until
            .filter(|_| self.bench.is_some() && total_attempts == 0);
        if let Some(until) = benched {
            // Every provider that passed the gates was benched: retry later, not a terminal posture
            // refusal.
            Err(Self::providers_benched_error(until))
        } else if let Some(refusal) = Self::entitlement_exhaustion(req, &throttle, total_attempts, pinned) {
            Err(refusal)
        } else if throttle.hidden_skipped && total_attempts == 0 {
            Err(ProviderError::ModelHidden {
                model: named.to_owned(),
            })
        } else if throttle.vision_skipped && total_attempts == 0 {
            Err(vision_unsupported_error())
        } else if req.min_privacy_tier > PrivacyTier::Unspecified && total_attempts == 0 {
            Err(tier_unavailable_error(req.min_privacy_tier))
        } else if req.zdr && total_attempts == 0 {
            Err(ProviderError::ZdrUnavailable(
                "no matching streaming provider deployment has verified ZDR support".to_owned(),
            ))
        } else if !req.min_residency.trim().is_empty() && total_attempts == 0 {
            Err(ProviderError::ResidencyViolation(format!(
                "no matching streaming provider deployment meets the requested `{}` residency floor",
                req.min_residency.trim()
            )))
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
        clock: &mut CallClock,
        org: Option<&OrgOverlay>,
    ) -> Option<mpsc::Receiver<InferChunk>> {
        // The platform or the organisation overlay hides this model: nothing is opened and the walk moves on, as on
        // the unary path.
        if self.platform_hides(model) || org_hides(org, model) {
            throttle.hidden_skipped = true;
            return None;
        }
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
            // Residency-floor gate on the streaming path, same as unary: an
            // operator-declared residency weaker than the requested floor is
            // skipped before any stream is opened.
            if residency_gate_blocks(req, caps.residency) {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    required = %req.min_residency,
                    declared = caps.residency.as_str(),
                    "stream provider skipped: residency floor not met"
                );
                continue;
            }
            let call_req = self.request_for(req, model, name);
            // As on the unary path: the model this provider would really serve is checked before any stream is opened.
            if self.platform_hides(&call_req.model) || org_hides(org, &call_req.model) {
                throttle.hidden_skipped = true;
                continue;
            }
            if let Some(reason) = req
                .access
                .as_ref()
                .and_then(|access| access.refuses(&call_req.model, name))
            {
                throttle.entitlement_denial.get_or_insert(reason);
                continue;
            }
            // Vision gate: a request that carries images is never sent to a
            // provider or model that cannot read them, and never answered from
            // its text alone. Skipped before any network call, like the gates
            // above; the exhaustion below names it.
            if !req.image_parts.is_empty() && !model_accepts_images(&caps, &call_req.model) {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    "provider skipped: the request carries images and this model does not accept them"
                );
                throttle.vision_skipped = true;
                continue;
            }
            if self.route_rules_skip(req, name, &caps, throttle) {
                continue;
            }
            if self.bench_skip(req, name, &call_req.model, &caps, throttle) {
                continue;
            }
            for attempt in 1..=self.max_retries {
                *total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_stream_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                );
                let began = clock.begin_attempt();
                // Instrumented, not entered across the await; see
                // `infer_one_model`.
                let outcome = provider
                    .infer_stream_dyn(call_req.as_ref())
                    .instrument(span.clone())
                    .await;
                if outcome.is_err() {
                    clock.attempt_failed(began);
                }
                let _enter = span.enter();
                let benched_now = self.bench_record(
                    req,
                    name,
                    &call_req.model,
                    &caps,
                    outcome.as_ref().err(),
                );

                match outcome {
                    Ok(rx) => {
                        let stream_clock = clock.accepted(began, *total_attempts);
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
                            stream_clock,
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
                    Err(ProviderError::TooLong { detail }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            "prompt too long for this provider; walking on for a larger window"
                        );
                        throttle.record_too_long(&detail);
                        break; // Deterministic for this provider — retrying cannot help.
                    }
                    Err(ProviderError::RequestRejected { detail }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            "provider rejected the request as malformed; walking on"
                        );
                        throttle.record_rejected(&detail);
                        break; // Deterministic for this provider — the same request is refused again.
                    }
                    Err(ProviderError::SubscriptionRefused {
                        reason,
                        retry_after_ms,
                        resets_at,
                    }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            reason = reason.code(),
                            resets_at = %reset_label(resets_at),
                            "subscription refused; not retrying"
                        );
                        throttle.record_refusal(ProviderError::SubscriptionRefused {
                            reason,
                            retry_after_ms,
                            resets_at,
                        });
                        return None;
                    }
                    // The broker answered the stream and had started the turn:
                    // one attempt, as for a refusal. Only a failure before the
                    // broker's `ready` (a transport fault) is retried below.
                    Err(ProviderError::SubscriptionFailed(message)) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            error = %message,
                            "subscription stream failed after its turn started; not retrying"
                        );
                        throttle.record_refusal(ProviderError::SubscriptionFailed(message));
                        return None;
                    }
                    Err(e) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            error = %e,
                            "provider stream attempt failed"
                        );
                        if benched_now {
                            break; // This failure benched the provider: no more attempts on it.
                        }
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
        let mut throttle = ThrottleState::default();
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
                // Instrumented, not entered across the await; see
                // `infer_one_model`.
                let outcome = provider
                    .create_embedding_dyn(req)
                    .instrument(span.clone())
                    .await;
                let _enter = span.enter();

                match outcome {
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
                        throttle.record(retry_after_ms);
                        break;
                    }
                    Err(ProviderError::TooLong { detail }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            "prompt too long for this provider; walking on for a larger window"
                        );
                        throttle.record_too_long(&detail);
                        break; // Deterministic for this provider — retrying cannot help.
                    }
                    Err(ProviderError::RequestRejected { detail }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            "provider rejected the request as malformed; walking on"
                        );
                        throttle.record_rejected(&detail);
                        break; // Deterministic for this provider — the same request is refused again.
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
            // Surfaces RateLimited (with retry-after) or TooLong when the walk
            // recorded one — an embedding input over the model limit was
            // previously reported as generic exhaustion, which callers retried.
            Err(throttle.exhausted_error(total_attempts))
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
        for m in ["claude-sonnet-5-5", "gpt-4o-mini"] {
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
        /// Declared residency. `Default` gives `Eu` so the pre-existing tests
        /// exercise routing, never the residency registration posture; the
        /// residency-floor and privacy-tier tests override it explicitly.
        residency: Residency,
        /// Which family this double stands in for.
        ///
        /// Routing reads the family from the provider's own declaration rather
        /// than from its registry name, so a double registered as
        /// `azure-anthropic` must *say* it is Anthropic-family or the chain will
        /// correctly refuse every `claude-*` model. `Default` gives
        /// `OpenAiCompatible`, so Claude-serving doubles set this explicitly.
        family: ModelFamily,
        /// Whether this double reads images. `Default` gives `false`, so the
        /// pre-existing tests never meet the vision gate.
        vision: bool,
    }

    impl Default for RecordingProvider {
        fn default() -> Self {
            Self {
                seen_model: Arc::new(Mutex::new(None)),
                zdr_supported: false,
                residency: Residency::Eu,
                family: ModelFamily::OpenAiCompatible,
                vision: false,
            }
        }
    }

    #[async_trait::async_trait]
    impl ProviderRouter for RecordingProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities {
                supports_vision: self.vision,
                supports_zdr: self.zdr_supported,
                model_family: self.family,
                residency: self.residency,
                ..crate::provider::ProviderCapabilities::default()
            }
        }

        async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
            *self.seen_model.lock().unwrap() = Some(req.model.clone());
            Ok(InferResponse {
                compaction_summary: String::new(),
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
            *self.seen_model.lock().unwrap() = Some(req.model.clone());
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }
    }

    fn chain_with(seen: Arc<Mutex<Option<String>>>) -> FallbackChain {
        let provider: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen,
            family: ModelFamily::Anthropic,
            ..RecordingProvider::default()
        });
        FallbackChain::new_with_providers(vec![("anthropic".to_owned(), provider)], 1)
    }

    #[test]
    fn route_note_switch_is_on_only_for_the_literal_true() {
        assert!(route_note_enabled_from(Some("true")));
        assert!(route_note_enabled_from(Some(" true ")));
        for raw in [None, Some(""), Some("1"), Some("on"), Some("TRUE")] {
            assert!(
                !route_note_enabled_from(raw),
                "{raw:?} must not turn the route note on"
            );
        }
    }

    #[test]
    fn constrained_posture_stamps_tier_shifted() {
        let policy = RoutingPolicy::default();
        let decision = intent::Decision {
            model: intent::choose(
                &policy,
                intent::VerevonMode::Balance,
                intent::Complexity::Simple,
                intent::BudgetPosture::Constrained,
            )
            .to_owned(),
            mode: intent::VerevonMode::Balance,
            complexity: intent::Complexity::Simple,
            posture: intent::BudgetPosture::Constrained,
            tier_shifted: true,
            effort_cap: None,
        };
        let note = budget_route_note(&policy, &decision, None);
        assert_eq!(note.budget_posture, intent::BudgetPosture::Constrained);
        assert!(note.tier_shifted, "balance one tier cheaper must be stated");
        assert!(!note.cheap_fallback_forced);
        assert_eq!(note.applied_effort, None);

        // Budget is already the floor: a constrained posture changes no model, so nothing is stated.
        let floor = intent::Decision {
            model: intent::choose(
                &policy,
                intent::VerevonMode::Budget,
                intent::Complexity::Simple,
                intent::BudgetPosture::Constrained,
            )
            .to_owned(),
            mode: intent::VerevonMode::Budget,
            complexity: intent::Complexity::Simple,
            posture: intent::BudgetPosture::Constrained,
            tier_shifted: false,
            effort_cap: None,
        };
        assert!(!budget_route_note(&policy, &floor, None).tier_shifted);
    }

    #[test]
    fn exhausted_stamps_cheap_fallback() {
        let policy = RoutingPolicy::default();
        let decision = intent::Decision {
            model: policy.cheap_fallback.clone(),
            mode: intent::VerevonMode::Genius,
            complexity: intent::Complexity::Complex,
            posture: intent::BudgetPosture::Exhausted,
            tier_shifted: false,
            effort_cap: Some(effort::Level::Low),
        };
        let note = budget_route_note(&policy, &decision, Some(effort::Level::Low));
        assert!(note.cheap_fallback_forced);
        assert!(!note.tier_shifted, "a forced fallback is not a tier move");
        assert_eq!(note.applied_effort, Some(effort::Level::Low));
    }

    #[test]
    fn plain_call_keeps_its_budget_note_and_a_role_call_adds_its_decision() {
        let budget = RouteNote {
            budget_posture: intent::BudgetPosture::Constrained,
            tier_shifted: true,
            cheap_fallback_forced: false,
            applied_effort: None,
            role: None,
            mode: Some(intent::VerevonMode::Balance),
            complexity: Some(intent::Complexity::Simple),
            reason: "budget_tier_shift",
        };
        // A call that is not a role call keeps its note exactly as the budget left it.
        assert_eq!(attach_role(Some(budget), None), Some(budget));
        assert_eq!(attach_role(None, None), None);

        let decision = role::RoleDecision {
            table_entry: None,
            source: role::RoleSource::TurnModel,
        };
        assert_eq!(
            attach_role(Some(budget), Some(decision)),
            Some(RouteNote {
                role: Some(decision),
                ..budget
            })
        );
        // A role call with no budget note still carries one: its decision is what it shows.
        assert_eq!(
            attach_role(None, Some(decision)),
            Some(RouteNote {
                role: Some(decision),
                ..RouteNote::unbudgeted()
            })
        );
    }

    #[tokio::test]
    async fn pinned_model_stamps_nothing() {
        let chain = FallbackChain::new_with_providers(Vec::new(), 0).with_intent_enabled(true);
        let pinned = InferRequest {
            request_id: "pinned".into(),
            model: "gpt-4o-mini".into(),
            ..Default::default()
        };
        assert!(chain.resolve_intent(&pinned, None).await.is_none());
    }

    #[tokio::test]
    async fn subscription_outage_never_reaches_an_api_provider_or_intent_ladder() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(503).set_body_json(serde_json::json!({"success":false,"error":{"code":"subscription_unavailable"}})))
            .mount(&server).await;
        let subscription: BoxedProvider = Arc::new(crate::provider::codex_subscription::CodexSubscriptionProvider::new(
            server.uri(),"test-key",vec!["gpt-5.6-terra".into()]).unwrap());
        let seen = Arc::new(Mutex::new(None));
        let alternative: BoxedProvider = Arc::new(RecordingProvider { seen_model:seen.clone(), ..Default::default() });
        let chain = FallbackChain::new_with_providers(vec![
            ("openai-codex-subscription".into(),subscription), ("azure-openai".into(),alternative)
        ],1).with_intent_enabled(true);
        let request = InferRequest { request_id:"subscription-only".into(),model:"gpt-5.6-terra".into(),
            provider_hint:"openai-codex-subscription".into(),subscription_connection_id:"connection".into(),
            org_id:"org".into(),user_id:"user".into(),..Default::default() };
        assert!(chain.infer(&request).await.is_err());
        assert!(chain.infer_stream(&request).await.is_err());
        assert!(seen.lock().unwrap().is_none(),"an unavailable subscription must never use another provider");
        for sent in server.received_requests().await.unwrap() {
            let body:serde_json::Value=serde_json::from_slice(&sent.body).unwrap();
            assert_eq!(body["model"],"gpt-5.6-terra");
            assert_eq!(body["connectionId"],"connection");
        }
    }

    /// Z-00, at the chain. A token that says ZDR used to reach this chain from
    /// the gateway with the user's subscription pin dropped (empty provider hint,
    /// empty connection id, `zdr` on), so the first ZDR-attested deployment
    /// behind it served the turn and the user's connection was replaced without
    /// a word. Both halves with the real subscription provider (which cannot
    /// give ZDR) beside an attested Azure double: the dropped-pin request lands
    /// on Azure, which is why the gateway refuses first; the pinned request the
    /// gateway's backstop forwards is refused here, never reaching Azure, the
    /// subscription's own endpoint or the intent ladder.
    #[tokio::test]
    async fn a_zdr_turn_pinned_to_the_subscription_never_lands_on_an_attested_azure() {
        let server = wiremock::MockServer::start().await;
        let subscription: BoxedProvider = Arc::new(
            crate::provider::codex_subscription::CodexSubscriptionProvider::new(
                server.uri(),
                "test-key",
                vec!["gpt-5.6-terra".into()],
            )
            .unwrap(),
        );
        let azure_seen = Arc::new(Mutex::new(None));
        let attested_azure: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: azure_seen.clone(),
            zdr_supported: true,
            ..Default::default()
        });
        let chain = FallbackChain::new_with_providers(
            vec![
                ("openai-codex-subscription".into(), subscription),
                ("azure-openai".into(), attested_azure),
            ],
            1,
        )
        .with_intent_enabled(true);

        // What the gateway used to send for a ZDR token on a Terra turn: the pin
        // is gone, so nothing says the subscription was ever asked for.
        let dropped_pin = InferRequest {
            request_id: "zdr-dropped-pin".into(),
            model: "gpt-5.6-terra".into(),
            org_id: "org".into(),
            user_id: "user".into(),
            zdr: true,
            ..Default::default()
        };
        chain
            .infer(&dropped_pin)
            .await
            .expect("an attested Azure serves a request that carries no pin");
        assert!(
            azure_seen.lock().unwrap().is_some(),
            "this is the silent re-route: the user's connection replaced by Azure"
        );

        // What the persistence-free path forwards now: the pin is kept.
        *azure_seen.lock().unwrap() = None;
        let pinned = InferRequest {
            request_id: "zdr-pinned".into(),
            provider_hint: "openai-codex-subscription".into(),
            subscription_connection_id: "connection".into(),
            ..dropped_pin
        };
        let refused = chain
            .infer(&pinned)
            .await
            .expect_err("a subscription cannot serve ZDR");
        assert!(matches!(refused, ProviderError::ZdrUnavailable(_)), "{refused:?}");
        let refused_stream = chain
            .infer_stream(&pinned)
            .await
            .expect_err("nor can it stream ZDR");
        assert!(matches!(refused_stream, ProviderError::ZdrUnavailable(_)), "{refused_stream:?}");
        assert!(
            azure_seen.lock().unwrap().is_none(),
            "a turn pinned to the subscription never lands on Azure, attested or not"
        );
        assert!(
            server.received_requests().await.unwrap().is_empty(),
            "and the subscription is not asked to serve what it cannot keep zero-retention"
        );
    }

    /// Refuses every call with one subscription refusal, counting the calls.
    struct RefusingProvider {
        calls: Arc<std::sync::atomic::AtomicUsize>,
        reason: crate::provider::SubscriptionRefusal,
    }

    impl RefusingProvider {
        fn refusal(&self) -> ProviderError {
            ProviderError::SubscriptionRefused {
                reason: self.reason,
                retry_after_ms: Some(52_260_000),
                resets_at: chrono::DateTime::parse_from_rfc3339("2026-09-26T01:21:00Z")
                    .ok()
                    .map(|at| at.with_timezone(&chrono::Utc)),
            }
        }
    }

    #[async_trait::async_trait]
    impl ProviderRouter for RefusingProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities::default()
        }

        async fn infer(&self, _req: &InferRequest) -> Result<InferResponse, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(self.refusal())
        }

        async fn infer_stream(
            &self,
            _req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(self.refusal())
        }
    }

    /// Throttles every call with a short, affordable retry-after, counting
    /// the calls: the case that would otherwise earn a waited second pass.
    struct ThrottledProvider {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl ProviderRouter for ThrottledProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities::default()
        }

        async fn infer(&self, _req: &InferRequest) -> Result<InferResponse, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ProviderError::RateLimited { retry_after_ms: 1_000 })
        }

        async fn infer_stream(
            &self,
            _req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ProviderError::RateLimited { retry_after_ms: 1_000 })
        }
    }

    fn assert_refused(
        outcome: Result<(), ProviderError>,
        reason: crate::provider::SubscriptionRefusal,
    ) {
        match outcome {
            Err(ProviderError::SubscriptionRefused {
                reason: got,
                retry_after_ms,
                resets_at,
            }) => {
                assert_eq!(got, reason);
                assert_eq!(retry_after_ms, Some(52_260_000));
                assert_eq!(
                    resets_at.map(|at| at.to_rfc3339()),
                    Some("2026-09-26T01:21:00+00:00".to_owned())
                );
            }
            other => panic!("expected the {reason:?} refusal back, got {other:?}"),
        }
    }

    /// A refusal is answered once, with no retry, no wait and no other
    /// provider, even with an unpinned request that another provider could
    /// serve and a throttled provider that would otherwise earn a waited
    /// second pass.
    #[tokio::test(start_paused = true)]
    async fn a_subscription_refusal_is_final_on_the_first_attempt() {
        use crate::provider::SubscriptionRefusal;
        use std::sync::atomic::{AtomicUsize, Ordering};
        for reason in [
            SubscriptionRefusal::UsageLimit,
            SubscriptionRefusal::ReauthRequired,
            SubscriptionRefusal::UpstreamUnavailable,
            SubscriptionRefusal::BadRequest,
        ] {
            let throttled_calls = Arc::new(AtomicUsize::new(0));
            let refused_calls = Arc::new(AtomicUsize::new(0));
            let seen = Arc::new(Mutex::new(None));
            let chain = FallbackChain::new_with_providers(
                vec![
                    (
                        "throttled".into(),
                        Arc::new(ThrottledProvider { calls: throttled_calls.clone() })
                            as BoxedProvider,
                    ),
                    (
                        "openai-codex-subscription".into(),
                        Arc::new(RefusingProvider { calls: refused_calls.clone(), reason })
                            as BoxedProvider,
                    ),
                    (
                        "azure-openai".into(),
                        Arc::new(RecordingProvider { seen_model: seen.clone(), ..Default::default() })
                            as BoxedProvider,
                    ),
                ],
                3,
            )
            .with_intent_enabled(true);
            let request = InferRequest {
                request_id: "refused".into(),
                model: "gpt-5.6-terra".into(),
                ..Default::default()
            };

            let began = tokio::time::Instant::now();
            assert_refused(chain.infer_without_response_cache(&request).await.map(drop), reason);
            assert_refused(chain.infer_stream(&request).await.map(drop), reason);
            assert_eq!(
                began.elapsed(),
                Duration::ZERO,
                "{reason:?}: a refusal is never waited out"
            );
            assert_eq!(refused_calls.load(Ordering::SeqCst), 2, "{reason:?}: one call per route");
            assert_eq!(throttled_calls.load(Ordering::SeqCst), 2, "{reason:?}: no second pass");
            assert!(
                seen.lock().unwrap().is_none(),
                "{reason:?}: no provider after the refusal may answer"
            );
        }
    }

    /// The live route: each typed broker refusal reaches the caller as its own
    /// typed error, after exactly one broker call per route, with no API
    /// provider and no intent-ladder rung ever asked.
    #[tokio::test]
    async fn every_broker_refusal_is_one_call_and_never_reaches_an_api_provider() {
        use crate::provider::SubscriptionRefusal;
        for (status, code, reason) in [
            (429, "subscription_usage_limit", SubscriptionRefusal::UsageLimit),
            (409, "subscription_reauth_required", SubscriptionRefusal::ReauthRequired),
            (503, "subscription_upstream_unavailable", SubscriptionRefusal::UpstreamUnavailable),
            (422, "subscription_bad_request", SubscriptionRefusal::BadRequest),
        ] {
            let server = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .respond_with(wiremock::ResponseTemplate::new(status).set_body_json(
                    serde_json::json!({"success": false, "error": {"code": code}}),
                ))
                .expect(2)
                .mount(&server)
                .await;
            let subscription: BoxedProvider = Arc::new(
                CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                    .unwrap(),
            );
            let seen = Arc::new(Mutex::new(None));
            let alternative: BoxedProvider =
                Arc::new(RecordingProvider { seen_model: seen.clone(), ..Default::default() });
            let chain = FallbackChain::new_with_providers(
                vec![
                    ("openai-codex-subscription".into(), subscription),
                    ("azure-openai".into(), alternative),
                ],
                3,
            )
            .with_intent_enabled(true);
            let request = InferRequest {
                request_id: "subscription-only".into(),
                model: "gpt-5.6-terra".into(),
                provider_hint: "openai-codex-subscription".into(),
                subscription_connection_id: "connection".into(),
                org_id: "org".into(),
                user_id: "user".into(),
                ..Default::default()
            };
            for outcome in [
                chain.infer(&request).await.map(drop),
                chain.infer_stream(&request).await.map(drop),
            ] {
                assert!(
                    matches!(outcome, Err(ProviderError::SubscriptionRefused { reason: got, .. }) if got == reason),
                    "{code}: {outcome:?}"
                );
            }
            assert!(seen.lock().unwrap().is_none(), "{code}: never another provider");
            // `expect(2)`: one unary and one stream call, no retries.
            server.verify().await;
        }
    }

    /// A stream the broker answered, then failed without a typed reason, is
    /// one broker call even with three attempts allowed: the Codex turn had
    /// started, so each retry would cost another turn of the plan's quota.
    /// Only a stream cut before the broker's `ready` (nothing ran yet) is
    /// retried. None of them reaches another provider.
    #[tokio::test]
    async fn an_untyped_stream_failure_after_ready_is_one_broker_call() {
        let ready = serde_json::json!({"type": "ready", "requestId": "untyped"});
        let cases: [(&str, Vec<serde_json::Value>, u64); 4] = [
            (
                "untyped error event",
                vec![
                    ready.clone(),
                    serde_json::json!({"type": "error", "code": "subscription_broker_failed"}),
                ],
                1,
            ),
            (
                "error event without a code",
                vec![ready.clone(), serde_json::json!({"type": "error"})],
                1,
            ),
            ("ended after ready", vec![ready.clone()], 1),
            ("ended before any event", Vec::new(), 3),
        ];
        for (case, events, broker_calls) in cases {
            let server = wiremock::MockServer::start().await;
            let body = events
                .iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect::<Vec<_>>()
                .concat();
            wiremock::Mock::given(wiremock::matchers::path(
                "/internal/model-subscriptions/openai-codex/infer/stream",
            ))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"),
            )
            .expect(broker_calls)
            .mount(&server)
            .await;
            let subscription: BoxedProvider = Arc::new(
                CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                    .unwrap(),
            );
            let seen = Arc::new(Mutex::new(None));
            let alternative: BoxedProvider =
                Arc::new(RecordingProvider { seen_model: seen.clone(), ..Default::default() });
            let chain = FallbackChain::new_with_providers(
                vec![
                    ("openai-codex-subscription".into(), subscription),
                    ("azure-openai".into(), alternative),
                ],
                3,
            )
            .with_intent_enabled(true);
            let request = InferRequest {
                request_id: "untyped".into(),
                model: "gpt-5.6-terra".into(),
                provider_hint: "openai-codex-subscription".into(),
                subscription_connection_id: "connection".into(),
                org_id: "org".into(),
                user_id: "user".into(),
                ..Default::default()
            };
            let outcome = chain.infer_stream(&request).await.map(drop);
            if broker_calls == 1 {
                assert!(
                    matches!(outcome, Err(ProviderError::SubscriptionFailed(_))),
                    "{case}: {outcome:?}"
                );
            } else {
                assert!(outcome.is_err(), "{case}: {outcome:?}");
            }
            assert!(seen.lock().unwrap().is_none(), "{case}: never another provider");
            server.verify().await;
        }
    }

    /// Whatever sits beside it, a failed subscription turn ends the walk on
    /// the first attempt, with no waited second pass for a throttled provider
    /// before it, and is what the caller gets back.
    #[tokio::test(start_paused = true)]
    async fn a_failed_subscription_turn_is_final_on_the_first_attempt() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct FailedTurnProvider {
            calls: Arc<AtomicUsize>,
        }

        #[async_trait::async_trait]
        impl ProviderRouter for FailedTurnProvider {
            fn capabilities(&self) -> crate::provider::ProviderCapabilities {
                crate::provider::ProviderCapabilities::default()
            }

            async fn infer(&self, _req: &InferRequest) -> Result<InferResponse, ProviderError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Err(ProviderError::SubscriptionFailed("turn failed".into()))
            }

            async fn infer_stream(
                &self,
                _req: &InferRequest,
            ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Err(ProviderError::SubscriptionFailed("turn failed".into()))
            }
        }

        let throttled_calls = Arc::new(AtomicUsize::new(0));
        let failed_calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(None));
        let chain = FallbackChain::new_with_providers(
            vec![
                (
                    "throttled".into(),
                    Arc::new(ThrottledProvider { calls: throttled_calls.clone() }) as BoxedProvider,
                ),
                (
                    "openai-codex-subscription".into(),
                    Arc::new(FailedTurnProvider { calls: failed_calls.clone() }) as BoxedProvider,
                ),
                (
                    "azure-openai".into(),
                    Arc::new(RecordingProvider { seen_model: seen.clone(), ..Default::default() })
                        as BoxedProvider,
                ),
            ],
            3,
        );
        let request = InferRequest {
            request_id: "failed-turn".into(),
            model: "gpt-5.6-terra".into(),
            ..Default::default()
        };
        let began = tokio::time::Instant::now();
        for outcome in [
            chain.infer_without_response_cache(&request).await.map(drop),
            chain.infer_stream(&request).await.map(drop),
        ] {
            assert!(
                matches!(&outcome, Err(ProviderError::SubscriptionFailed(message)) if message == "turn failed"),
                "{outcome:?}"
            );
        }
        assert_eq!(began.elapsed(), Duration::ZERO, "a failed turn is never waited out");
        assert_eq!(failed_calls.load(Ordering::SeqCst), 2, "one call per route");
        assert_eq!(throttled_calls.load(Ordering::SeqCst), 2, "no second pass");
        assert!(seen.lock().unwrap().is_none(), "no provider after the failed turn");
    }

    #[test]
    fn a_recorded_refusal_outranks_throttling_and_overflow_at_exhaustion() {
        let mut throttle = ThrottleState::default();
        throttle.record(2_000);
        throttle.record_too_long("overflow");
        throttle.record_refusal(ProviderError::SubscriptionRefused {
            reason: crate::provider::SubscriptionRefusal::ReauthRequired,
            retry_after_ms: None,
            resets_at: None,
        });
        // The first refusal is the one kept.
        throttle.record_refusal(ProviderError::SubscriptionRefused {
            reason: crate::provider::SubscriptionRefusal::UsageLimit,
            retry_after_ms: None,
            resets_at: None,
        });
        assert!(throttle.refused());
        assert!(matches!(
            throttle.exhausted_error(3),
            ProviderError::SubscriptionRefused {
                reason: crate::provider::SubscriptionRefusal::ReauthRequired,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn subscription_usage_and_broker_phases_survive_the_chain() {
        use wiremock::matchers::path;
        let usage = serde_json::json!({
            "inputTokens": 1800, "cachedInputTokens": 1024, "cacheWriteInputTokens": 256,
            "outputTokens": 120, "reasoningOutputTokens": 40
        });
        let phases = serde_json::json!({"spawn": 41, "turn_total": 9000});
        let server = wiremock::MockServer::start().await;
        let reply = serde_json::json!({"success": true, "data": {"response": {
            "requestId": "usage", "content": "Hei", "modelUsed": "gpt-5.6-terra",
            "usage": usage, "phasesMs": phases
        }}});
        wiremock::Mock::given(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(reply))
            .mount(&server)
            .await;
        let done_event = serde_json::json!({
            "type": "done", "requestId": "usage", "modelUsed": "gpt-5.6-terra",
            "usage": usage, "phasesMs": phases
        });
        let stream = [
            serde_json::json!({"type": "ready", "requestId": "usage"}),
            serde_json::json!({"type": "delta", "delta": "Hei"}),
            done_event,
        ]
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<Vec<_>>()
        .concat();
        wiremock::Mock::given(path(
            "/internal/model-subscriptions/openai-codex/infer/stream",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_raw(stream, "text/event-stream"),
        )
        .mount(&server)
        .await;
        let subscription: BoxedProvider = Arc::new(
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap(),
        );
        let chain = FallbackChain::new_with_providers(
            vec![("openai-codex-subscription".into(), subscription)],
            1,
        );
        let request = InferRequest {
            request_id: "usage".into(),
            model: "gpt-5.6-terra".into(),
            provider_hint: "openai-codex-subscription".into(),
            subscription_connection_id: "connection".into(),
            org_id: "org".into(),
            user_id: "user".into(),
            ..Default::default()
        };

        let response = chain.infer_without_response_cache(&request).await.unwrap();
        assert_eq!(
            (response.input_tokens, response.output_tokens),
            (1_800, 120)
        );
        assert_eq!(
            (
                response.cache_read_input_tokens,
                response.cache_creation_input_tokens
            ),
            (1_024, 256)
        );
        let timing = response
            .provider_timing
            .expect("the chain stamps the timing");
        assert!(timing.usage_reported);
        assert_eq!(timing.attempts, 1);
        assert_eq!(
            timing.upstream.get("turn_total"),
            Some(&Duration::from_secs(9))
        );

        let mut chunks = chain.infer_stream(&request).await.unwrap();
        let mut done = None;
        while let Some(chunk) = chunks.recv().await {
            if chunk.done {
                done = Some(chunk);
                break;
            }
            assert!(
                chunk.provider_timing.is_none(),
                "only the final chunk carries timing"
            );
        }
        let done = done.expect("a final chunk");
        assert_eq!((done.input_tokens, done.output_tokens), (1_800, 120));
        assert_eq!(done.provider_used, "openai-codex-subscription");
        let timing = done.provider_timing.expect("the chain stamps the timing");
        assert!(timing.usage_reported);
        assert_eq!(
            timing.upstream.get("spawn"),
            Some(&Duration::from_millis(41))
        );
        assert!(timing.upstream.contains_key("broker_ready"));
    }

    #[tokio::test]
    async fn the_subscription_route_is_stamped_global_and_cannot_meet_an_eu_or_zdr_floor() {
        // The provenance the browser's answer meta shows ("Behandlet: global,
        // ingen EU- eller ZDR-garanti") is stamped here, not by the provider:
        // the provider returns both fields empty, and the chain writes them
        // from the provider's declared capabilities on the unary response and
        // on the final stream chunk alike.
        use wiremock::matchers::path;
        let usage = serde_json::json!({"inputTokens": 10, "outputTokens": 2});
        let server = wiremock::MockServer::start().await;
        let reply = serde_json::json!({"success": true, "data": {"response": {
            "requestId": "stamp", "content": "Hei", "modelUsed": "gpt-5.6-terra", "usage": usage
        }}});
        wiremock::Mock::given(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(reply))
            .mount(&server)
            .await;
        let stream = [
            serde_json::json!({"type": "ready", "requestId": "stamp"}),
            serde_json::json!({"type": "delta", "delta": "Hei"}),
            serde_json::json!({
                "type": "done", "requestId": "stamp", "modelUsed": "gpt-5.6-terra", "usage": usage
            }),
        ]
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<Vec<_>>()
        .concat();
        wiremock::Mock::given(path(
            "/internal/model-subscriptions/openai-codex/infer/stream",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_raw(stream, "text/event-stream"),
        )
        .mount(&server)
        .await;
        let provider =
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap();

        // What the provider declares: no region commitment, no verified ZDR.
        let caps = ProviderRouter::capabilities(&provider);
        assert_eq!(caps.residency, Residency::Global);
        assert!(!caps.supports_zdr);
        assert_eq!(PrivacyTier::classify(&caps), PrivacyTier::Global);

        let request = InferRequest {
            request_id: "stamp".into(),
            model: "gpt-5.6-terra".into(),
            provider_hint: "openai-codex-subscription".into(),
            subscription_connection_id: "connection".into(),
            org_id: "org".into(),
            user_id: "user".into(),
            ..Default::default()
        };
        let raw = ProviderRouter::infer(&provider, &request).await.unwrap();
        assert_eq!(
            (raw.provider_used.as_str(), raw.residency.as_str()),
            ("", ""),
            "the provider stamps nothing itself; the chain does"
        );

        let subscription: BoxedProvider = Arc::new(provider);
        let chain = FallbackChain::new_with_providers(
            vec![("openai-codex-subscription".into(), subscription)],
            1,
        );
        let response = chain.infer_without_response_cache(&request).await.unwrap();
        assert_eq!(response.provider_used, "openai-codex-subscription");
        assert_eq!(response.residency, "global");

        let mut chunks = chain.infer_stream(&request).await.unwrap();
        let mut done = None;
        while let Some(chunk) = chunks.recv().await {
            if chunk.done {
                done = Some(chunk);
                break;
            }
        }
        let done = done.expect("a final chunk");
        assert_eq!(done.provider_used, "openai-codex-subscription");
        assert_eq!(done.residency, "global");

        // The same declaration refuses what the stamp does not promise: an
        // EU or ZDR requirement is never answered through this route.
        for floor in [PrivacyTier::EuResident, PrivacyTier::ZdrContractual] {
            let strict = InferRequest {
                min_privacy_tier: floor,
                ..request.clone()
            };
            assert!(
                matches!(
                    chain.infer_without_response_cache(&strict).await,
                    Err(ProviderError::TierUnavailable(_))
                ),
                "unary: {floor:?} must be refused"
            );
            assert!(
                matches!(
                    chain.infer_stream(&strict).await,
                    Err(ProviderError::TierUnavailable(_))
                ),
                "stream: {floor:?} must be refused"
            );
        }
    }

    #[tokio::test]
    async fn fresh_inference_neither_reads_nor_writes_response_cache_or_bypasses_zdr() {
        let seen = Arc::new(Mutex::new(None));
        let chain = chain_with(seen.clone());
        let mut request = InferRequest { model: "claude-sonnet-4-6".into(), ..Default::default() };
        for id in ["fresh-1", "fresh-2"] {
            request.request_id = id.into();
            assert_eq!(chain.infer_without_response_cache(&request).await.unwrap().request_id, id);
            assert_eq!(chain.cache.len(), 0);
        }
        request.request_id = "cached".into();
        chain.infer(&request).await.unwrap();
        assert_eq!(chain.cache.len(), 1);
        request.request_id = "fresh-3".into();
        assert_eq!(chain.infer_without_response_cache(&request).await.unwrap().request_id, "fresh-3");
        // The original entry was neither read nor overwritten by fresh-3.
        assert_eq!(chain.infer(&request).await.unwrap().request_id, "cached");
        *seen.lock().unwrap() = None;
        request.zdr = true;
        assert!(chain.infer_without_response_cache(&request).await.is_err());
        assert!(seen.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn resolves_unspecified_model_to_provider_default() {
        let seen = Arc::new(Mutex::new(None));
        let chain = chain_with(seen.clone());
        let req = InferRequest {
            thinking_budget_tokens: 0,
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
            thinking_budget_tokens: 0,
            request_id: "r2".to_owned(),
            model: "claude-opus-5-5".to_owned(),
            ..Default::default()
        };
        chain.infer(&req).await.unwrap();
        assert_eq!(
            seen.lock().unwrap().as_deref(),
            Some("claude-opus-5-5")
        );
    }

    #[tokio::test]
    async fn a_retired_pinned_model_is_answered_by_its_successor() {
        let seen = Arc::new(Mutex::new(None));
        let chain = chain_with(seen.clone());
        let req = InferRequest {
            thinking_budget_tokens: 0,
            request_id: "r2-retired".to_owned(),
            model: "claude-opus-4-20250514".to_owned(),
            ..Default::default()
        };
        let response = chain.infer(&req).await.unwrap();
        assert_eq!(seen.lock().unwrap().as_deref(), Some("claude-opus-5-5"));
        assert_eq!(response.model_used, "claude-opus-5-5");
    }

    #[tokio::test]
    async fn a_retired_model_and_its_successor_share_one_response_cache_entry() {
        let seen = Arc::new(Mutex::new(None));
        let chain = chain_with(seen.clone());
        let successor = InferRequest {
            thinking_budget_tokens: 0,
            request_id: "cache-successor".to_owned(),
            model: "claude-sonnet-5-5".to_owned(),
            ..Default::default()
        };
        chain.infer(&successor).await.unwrap();
        // A call that reached the provider would record its model here, so the
        // sentinel shows that the retired-id request was answered from the cache.
        *seen.lock().unwrap() = Some("sentinel".to_owned());
        let retired = InferRequest {
            request_id: "cache-retired".to_owned(),
            model: "claude-sonnet-4-20250514".to_owned(),
            ..successor.clone()
        };
        let response = chain.infer(&retired).await.unwrap();
        assert_eq!(response.request_id, "cache-successor");
        assert_eq!(seen.lock().unwrap().as_deref(), Some("sentinel"));
    }

    #[tokio::test]
    async fn the_streaming_path_answers_a_retired_model_with_its_successor() {
        let seen = Arc::new(Mutex::new(None));
        let chain = chain_with(seen.clone());
        let req = InferRequest {
            thinking_budget_tokens: 0,
            request_id: "stream-retired".to_owned(),
            model: "claude-3-5-haiku-20241022".to_owned(),
            ..Default::default()
        };
        chain.infer_stream(&req).await.unwrap();
        assert_eq!(seen.lock().unwrap().as_deref(), Some("claude-haiku-5-5"));
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
            thinking_budget_tokens: 0,
            request_id: "r3".to_owned(),
            model: "verevon-budget".to_owned(),
            messages: vec![crate::provider::ChatMessage {
                compaction_summary: String::new(),
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
            thinking_budget_tokens: 0,
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
            thinking_budget_tokens: 0,
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

        let resp = chain
            .infer(&req)
            .await
            .expect("the EU provider satisfies the minimum");
        assert!(
            weak_seen.lock().unwrap().is_none(),
            "a provider below the requested tier must be skipped BEFORE any call"
        );
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
        let chain = FallbackChain::new_with_providers(vec![("openai".to_owned(), provider)], 1);
        let req = InferRequest {
            request_id: "tier-u2".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            ..Default::default()
        };
        chain
            .infer(&req)
            .await
            .expect("no constraint means no skipping");
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
            Err(ProviderError::UnsupportedModel("stream double".to_owned()))
        }

        async fn infer_stream(
            &self,
            req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            *self.reached.lock().unwrap() = true;
            let (tx, rx) = mpsc::channel(1);
            let chunk = InferChunk {
                compaction_summary: String::new(),
                request_id: req.request_id.clone(),
                delta: String::new(),
                done: true,
                model_used: req.model.clone(),
                input_tokens: 1,
                output_tokens: 1,
                // A final chunk on the merged contract carries the honest
                // termination signal and the (empty) reasoning channel too.
                stop_reason: "end_turn".to_owned(),
                reasoning_delta: String::new(),
                provider_used: String::new(),
                residency: String::new(),
                token_confidence: None,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
                provider_timing: None,
                service_tier_used: String::new(),
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
        assert!(
            !*weak_reached.lock().unwrap(),
            "the weak provider must be skipped before opening any stream"
        );
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
        assert!(
            matches!(err, ProviderError::TierUnavailable(message) if message.contains("eu_resident"))
        );
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
        assert!(
            !*reached.lock().unwrap(),
            "embedding provider below the tier must never see a network call"
        );
        assert!(matches!(
            err,
            ProviderError::TierUnavailable(message) if message.contains("eu_resident")
        ));

        // And with no constraint, the very same deployment serves fine.
        let unconstrained = EmbedRequest {
            min_privacy_tier: PrivacyTier::Unspecified,
            ..req
        };
        chain
            .create_embedding(&unconstrained)
            .await
            .expect("Unspecified imposes no constraint");
        assert!(*reached.lock().unwrap());
    }

    // ---------------------------------------------------------------------
    // Residency-floor gating — the `min_residency` half of the same
    // provider-selection contract as the tier tests above. Kept as its own
    // section because the floor is expressed as an operator TOKEN (parsed,
    // typo-rejecting) rather than as a tier enum, and it fails closed with
    // `ResidencyViolation` instead of `TierUnavailable`.
    // ---------------------------------------------------------------------

    #[tokio::test]
    async fn residency_floor_skips_a_provider_declaring_weaker_residency() {
        let global_seen = Arc::new(Mutex::new(None));
        let eu_seen = Arc::new(Mutex::new(None));
        let global: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: global_seen.clone(),
            residency: Residency::Global,
            ..RecordingProvider::default()
        });
        let eu: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: eu_seen.clone(),
            residency: Residency::Eu,
            ..RecordingProvider::default()
        });
        let chain = FallbackChain::new_with_providers(
            vec![
                ("openai".to_owned(), global),
                ("azure-openai".to_owned(), eu),
            ],
            1,
        );
        let req = InferRequest {
            thinking_budget_tokens: 0,
            request_id: "res-1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            min_residency: "eu".to_owned(),
            ..Default::default()
        };

        chain
            .infer(&req)
            .await
            .expect("EU provider meets the floor");
        assert!(
            global_seen.lock().unwrap().is_none(),
            "the Global provider must never be reached once an EU floor is requested"
        );
        assert!(eu_seen.lock().unwrap().is_some());
    }

    #[tokio::test]
    async fn residency_floor_fails_closed_without_a_compliant_route() {
        let seen = Arc::new(Mutex::new(None));
        let global: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen.clone(),
            residency: Residency::Global,
            ..RecordingProvider::default()
        });
        let chain = FallbackChain::new_with_providers(vec![("openai".to_owned(), global)], 1);
        let req = InferRequest {
            thinking_budget_tokens: 0,
            request_id: "res-2".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            min_residency: "norway".to_owned(),
            ..Default::default()
        };

        let error = chain.infer(&req).await.unwrap_err();
        assert!(matches!(error, ProviderError::ResidencyViolation(_)));
        assert!(seen.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn an_unrecognized_residency_token_is_rejected_before_any_provider_is_tried() {
        let seen = Arc::new(Mutex::new(None));
        let provider: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen.clone(),
            ..RecordingProvider::default()
        });
        let chain = FallbackChain::new_with_providers(vec![("openai".to_owned(), provider)], 1);
        let req = InferRequest {
            thinking_budget_tokens: 0,
            request_id: "res-3".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            min_residency: "atlantis".to_owned(),
            ..Default::default()
        };

        let error = chain.infer(&req).await.unwrap_err();
        assert!(matches!(error, ProviderError::ResidencyViolation(_)));
        assert!(
            seen.lock().unwrap().is_none(),
            "a typo'd token must fail before any provider is even tried"
        );
    }

    #[tokio::test]
    async fn the_streaming_path_also_honors_the_residency_floor() {
        let global_seen = Arc::new(Mutex::new(None));
        let norway_seen = Arc::new(Mutex::new(None));
        let global: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: global_seen.clone(),
            residency: Residency::Global,
            ..RecordingProvider::default()
        });
        let norway: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: norway_seen.clone(),
            residency: Residency::Norway,
            ..RecordingProvider::default()
        });
        let chain = FallbackChain::new_with_providers(
            vec![
                ("openai".to_owned(), global),
                ("azure-openai".to_owned(), norway),
            ],
            1,
        );
        let req = InferRequest {
            thinking_budget_tokens: 0,
            request_id: "res-4".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            min_residency: "sovereign".to_owned(),
            ..Default::default()
        };

        chain
            .infer_stream(&req)
            .await
            .expect("the Norway-resident provider meets a sovereign floor");
        assert!(global_seen.lock().unwrap().is_none());
        assert!(norway_seen.lock().unwrap().is_some());
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

    /// The typed overflow must survive exhaustion. Before this state existed,
    /// `classify_http_failure` produced `TooLong` at five provider call sites
    /// and every chain discarded it in a generic `Err(e) => warn!` arm — so
    /// `too_long_status` and the `x-mp-provider-error: too_long` trailer had
    /// ZERO production callers, and the gateway's context-length recovery,
    /// keyed on that trailer, could never trigger from the typed path.
    #[test]
    fn an_exhausted_walk_that_saw_too_long_reports_too_long_not_generic_exhaustion() {
        let mut throttle = ThrottleState::default();
        throttle.record_too_long("400: prompt is 250000 tokens, maximum is 128000");
        match throttle.exhausted_error(4) {
            ProviderError::TooLong { detail } => {
                assert!(
                    detail.contains("250000"),
                    "the provider's own text survives"
                );
            }
            other => panic!("expected TooLong, got {other:?}"),
        }
        // First detail wins — the walk's earliest overflow names the model the
        // request was actually routed to.
        let mut throttle = ThrottleState::default();
        throttle.record_too_long("first");
        throttle.record_too_long("second");
        assert!(matches!(
            throttle.exhausted_error(2),
            ProviderError::TooLong { detail } if detail == "first"
        ));
    }

    /// RateLimited outranks TooLong on purpose: a throttled provider might
    /// genuinely serve this exact prompt after the wait (larger window,
    /// different limit), so "try again shortly" is still the true story; the
    /// overflow only becomes the answer when no retry can help.
    #[test]
    fn a_throttled_walk_outranks_the_overflow() {
        let mut throttle = ThrottleState::default();
        throttle.record_too_long("overflow");
        throttle.record(2_000);
        assert!(matches!(
            throttle.exhausted_error(3),
            ProviderError::RateLimited {
                retry_after_ms: 2_000
            }
        ));
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

    /// Refuses every call as a malformed request, counting the calls. This is the shape
    /// the Anthropic adapter gives a 400 that is not an overflow.
    struct MalformedRequestProvider {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl MalformedRequestProvider {
        fn refusal(&self) -> ProviderError {
            ProviderError::RequestRejected {
                detail: "400 Bad Request: tools.0.custom.name: String should match pattern"
                    .to_owned(),
            }
        }
    }

    #[async_trait::async_trait]
    impl ProviderRouter for MalformedRequestProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities::default()
        }

        async fn infer(&self, _req: &InferRequest) -> Result<InferResponse, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(self.refusal())
        }

        async fn infer_stream(
            &self,
            _req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(self.refusal())
        }

        async fn create_embedding(
            &self,
            _req: &EmbedRequest,
        ) -> Result<EmbedResponse, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(self.refusal())
        }
    }

    /// Fails every call with a server error, counting the calls.
    struct ServerErrorProvider {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl ProviderRouter for ServerErrorProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities::default()
        }

        async fn infer(&self, _req: &InferRequest) -> Result<InferResponse, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ProviderError::Http("500 Internal Server Error: upstream".to_owned()))
        }

        async fn infer_stream(
            &self,
            _req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ProviderError::Http("500 Internal Server Error: upstream".to_owned()))
        }
    }

    /// A request its provider refuses as malformed is sent to that provider once on each
    /// route, not once per retry, because the same request is refused again. The chain
    /// then walks on to the next provider, which serves it.
    #[tokio::test]
    async fn a_malformed_request_is_sent_once_per_provider_and_the_chain_walks_on() {
        use std::sync::atomic::Ordering;
        let refused = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(None));
        let chain = FallbackChain::new_with_providers(
            vec![
                (
                    "azure-openai".into(),
                    Arc::new(MalformedRequestProvider { calls: refused.clone() }) as BoxedProvider,
                ),
                (
                    "openai".into(),
                    Arc::new(RecordingProvider { seen_model: seen.clone(), ..Default::default() })
                        as BoxedProvider,
                ),
            ],
            3,
        );
        let request = InferRequest {
            request_id: "malformed".into(),
            model: "gpt-4o-mini".into(),
            ..Default::default()
        };

        let response = chain.infer(&request).await.expect("the second provider serves it");
        assert_eq!(response.provider_used, "openai");
        assert_eq!(refused.load(Ordering::SeqCst), 1, "one unary attempt, not three");

        chain.infer_stream(&request).await.expect("the second provider streams it");
        assert_eq!(refused.load(Ordering::SeqCst), 2, "one stream attempt, not three");
        assert!(seen.lock().unwrap().is_some());
    }

    /// A walk that only ever saw a refusal reports the refusal, not generic exhaustion,
    /// and still makes one attempt.
    #[tokio::test]
    async fn a_walk_that_only_saw_a_refusal_reports_the_refusal() {
        use std::sync::atomic::Ordering;
        let refused = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let chain = FallbackChain::new_with_providers(
            vec![(
                "azure-openai".into(),
                Arc::new(MalformedRequestProvider { calls: refused.clone() }) as BoxedProvider,
            )],
            3,
        );
        let request = InferRequest {
            request_id: "refused-only".into(),
            model: "gpt-4o-mini".into(),
            ..Default::default()
        };

        match chain.infer(&request).await {
            Err(ProviderError::RequestRejected { detail }) => {
                assert!(detail.contains("tools.0.custom.name"), "{detail}");
            }
            other => panic!("expected the refusal back, got {other:?}"),
        }
        assert_eq!(refused.load(Ordering::SeqCst), 1);
    }

    /// The embedding walk types a refusal the same way: one attempt, then the chain walks on.
    #[tokio::test]
    async fn a_refused_embedding_input_is_sent_once_per_provider() {
        use std::sync::atomic::Ordering;
        let refused = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let chain = FallbackChain::new_with_providers(
            vec![(
                "azure-openai".to_owned(),
                Arc::new(MalformedRequestProvider { calls: refused.clone() }) as BoxedProvider,
            )],
            3,
        );
        let request = EmbedRequest {
            request_id: "embed-refused".to_owned(),
            provider_hint: "azure-openai".to_owned(),
            text: "hello".to_owned(),
            model: "text-embedding-3-large".to_owned(),
            region: "swedencentral".to_owned(),
            ..Default::default()
        };

        assert!(matches!(
            chain.create_embedding(&request).await,
            Err(ProviderError::RequestRejected { .. })
        ));
        assert_eq!(refused.load(Ordering::SeqCst), 1);
    }

    /// A failure a retry can clear keeps its retries: a 5xx is three attempts on the same
    /// provider before the chain gives up on it.
    #[tokio::test]
    async fn a_server_error_is_still_retried_on_the_same_provider() {
        use std::sync::atomic::Ordering;
        let failing = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let chain = FallbackChain::new_with_providers(
            vec![(
                "azure-openai".into(),
                Arc::new(ServerErrorProvider { calls: failing.clone() }) as BoxedProvider,
            )],
            3,
        );
        let request = InferRequest {
            request_id: "server-error".into(),
            model: "gpt-4o-mini".into(),
            ..Default::default()
        };

        assert!(matches!(
            chain.infer(&request).await,
            Err(ProviderError::AllExhausted { attempts: 3 })
        ));
        assert_eq!(failing.load(Ordering::SeqCst), 3);
    }

    /// A refusal is reported only when nothing more specific happened. Throttling and an
    /// overflow outrank it, and the first refusal is the one named.
    #[test]
    fn a_refusal_is_reported_only_when_nothing_more_specific_happened() {
        let mut throttle = ThrottleState::default();
        throttle.record_rejected("400: tools.0.custom.name");
        assert!(!throttle.throttled(), "a refusal is never waited out");
        assert!(matches!(
            throttle.exhausted_error(2),
            ProviderError::RequestRejected { detail } if detail == "400: tools.0.custom.name"
        ));
        throttle.record_too_long("413: too long");
        assert!(matches!(throttle.exhausted_error(2), ProviderError::TooLong { .. }));
        throttle.record(2_000);
        assert!(matches!(
            throttle.exhausted_error(2),
            ProviderError::RateLimited { retry_after_ms: 2_000 }
        ));

        let mut first = ThrottleState::default();
        first.record_rejected("first");
        first.record_rejected("second");
        assert!(matches!(
            first.exhausted_error(1),
            ProviderError::RequestRejected { detail } if detail == "first"
        ));
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
                compaction_summary: String::new(),
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
            thinking_budget_tokens: 0,
            request_id: request_id.to_owned(),
            model: model.to_owned(),
            messages: vec![crate::provider::ChatMessage {
                compaction_summary: String::new(),
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
                compaction_summary: String::new(),
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
                        compaction_summary: String::new(),
                        request_id: request_id.clone(),
                        delta: "hei".to_owned(),
                        done: false,
                        model_used: String::new(),
                        input_tokens: 0,
                        output_tokens: 0,
                        stop_reason: String::new(),
                        reasoning_delta: String::new(),
                        provider_used: String::new(),
                        residency: String::new(),
                        token_confidence: None,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                        provider_timing: None,
                        service_tier_used: String::new(),
                    })
                    .await;
                let _ = tx
                    .send(InferChunk {
                        compaction_summary: String::new(),
                        request_id,
                        delta: String::new(),
                        done: true,
                        model_used: versioned,
                        input_tokens: 3,
                        output_tokens: 1,
                        stop_reason: "end_turn".to_owned(),
                        reasoning_delta: String::new(),
                        provider_used: String::new(),
                        residency: String::new(),
                        token_confidence: None,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                        provider_timing: None,
                        service_tier_used: String::new(),
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
                thinking_budget_tokens: 0,
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
                thinking_budget_tokens: 0,
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

    /// Records the reasoning effort and the legacy budget each call reached it
    /// with, so the effort-ceiling tests can see what the policy let through.
    struct EffortRecordingProvider {
        seen: Arc<Mutex<Vec<(Option<effort::Level>, i32)>>>,
    }

    #[async_trait::async_trait]
    impl ProviderRouter for EffortRecordingProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities {
                model_family: ModelFamily::OpenAiCompatible,
                residency: Residency::Eu,
                ..crate::provider::ProviderCapabilities::default()
            }
        }

        async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
            self.seen
                .lock()
                .unwrap()
                .push((req.reasoning_effort, req.thinking_budget_tokens));
            Ok(InferResponse {
                request_id: req.request_id.clone(),
                content: "ok".to_owned(),
                model_used: req.model.clone(),
                stop_reason: "stop".to_owned(),
                ..InferResponse::default()
            })
        }

        async fn infer_stream(
            &self,
            req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            self.seen
                .lock()
                .unwrap()
                .push((req.reasoning_effort, req.thinking_budget_tokens));
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }
    }

    /// An org policy as org-core answers it, whose only limit is the `max_effort`
    /// ceiling. `enforced` picks enforce over shadow.
    fn effort_ceiling_access(enforced: bool, max_effort: &str) -> Arc<RequestAccess> {
        let body = serde_json::json!({
            "state": "org",
            "revision": 1,
            "policy": {"allow_mode": "all", "allow": [], "deny": [], "max_effort": max_effort},
        });
        let mode = if enforced {
            mp_contracts::model_entitlement::Mode::Enforce
        } else {
            mp_contracts::model_entitlement::Mode::Shadow
        };
        let resolved = mp_contracts::model_entitlement::ResolvedAccess::from_body(&body)
            .expect("a readable policy body");
        Arc::new(RequestAccess::new(Arc::new(resolved), mode, "org-1"))
    }

    /// A request for a model every policy here allows, carrying the given effort
    /// level, legacy budget and access.
    fn effort_probe(
        id: &str,
        reasoning_effort: Option<effort::Level>,
        thinking_budget_tokens: i32,
        access: Option<Arc<RequestAccess>>,
    ) -> InferRequest {
        InferRequest {
            request_id: id.to_owned(),
            model: "gpt-5-mini".to_owned(),
            reasoning_effort,
            thinking_budget_tokens,
            access,
            ..Default::default()
        }
    }

    /// What the provider was handed by one unary call through a fresh chain.
    async fn effort_reached_unary(request: &InferRequest) -> Vec<(Option<effort::Level>, i32)> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let provider: BoxedProvider = Arc::new(EffortRecordingProvider { seen: seen.clone() });
        let chain =
            FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), provider)], 1);
        chain.infer(request).await.unwrap();
        let reached = seen.lock().unwrap().clone();
        reached
    }

    /// The same request through the streaming path.
    async fn effort_reached_streamed(request: &InferRequest) -> Vec<(Option<effort::Level>, i32)> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let provider: BoxedProvider = Arc::new(EffortRecordingProvider { seen: seen.clone() });
        let chain =
            FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), provider)], 1);
        let _rx = chain.infer_stream(request).await.unwrap();
        let reached = seen.lock().unwrap().clone();
        reached
    }

    #[tokio::test]
    async fn an_enforced_effort_ceiling_lowers_the_level_on_the_unary_path() {
        let request = effort_probe(
            "effort-unary-level",
            Some(effort::Level::High),
            0,
            Some(effort_ceiling_access(true, "low")),
        );
        assert_eq!(
            effort_reached_unary(&request).await,
            [(Some(effort::Level::Low), 0)]
        );
    }

    #[tokio::test]
    async fn an_enforced_effort_ceiling_caps_the_legacy_budget_when_no_level_is_set() {
        let request = effort_probe(
            "effort-unary-budget",
            None,
            4096,
            Some(effort_ceiling_access(true, "low")),
        );
        assert_eq!(effort_reached_unary(&request).await, [(None, 1024)]);
    }

    #[tokio::test]
    async fn an_enforced_effort_ceiling_applies_on_the_streaming_path_too() {
        let level = effort_probe(
            "effort-stream-level",
            Some(effort::Level::High),
            0,
            Some(effort_ceiling_access(true, "low")),
        );
        assert_eq!(
            effort_reached_streamed(&level).await,
            [(Some(effort::Level::Low), 0)]
        );
        let budget = effort_probe(
            "effort-stream-budget",
            None,
            4096,
            Some(effort_ceiling_access(true, "low")),
        );
        assert_eq!(effort_reached_streamed(&budget).await, [(None, 1024)]);
    }

    #[tokio::test]
    async fn a_shadowed_or_absent_policy_leaves_the_effort_exactly_as_asked() {
        let shadow = Some(effort_ceiling_access(false, "low"));
        let level = effort_probe(
            "effort-shadow-level",
            Some(effort::Level::High),
            0,
            shadow.clone(),
        );
        assert_eq!(
            effort_reached_unary(&level).await,
            [(Some(effort::Level::High), 0)]
        );
        let budget = effort_probe("effort-shadow-budget", None, 4096, shadow);
        assert_eq!(effort_reached_unary(&budget).await, [(None, 4096)]);
        // A service principal carries no access, so no policy ever clamps it.
        let service = effort_probe(
            "effort-service-principal",
            Some(effort::Level::High),
            0,
            None,
        );
        assert_eq!(
            effort_reached_unary(&service).await,
            [(Some(effort::Level::High), 0)]
        );
    }

    #[tokio::test]
    async fn an_effort_ceiling_never_raises_and_ignores_an_empty_or_unknown_name() {
        for (index, ceiling) in ["", "ultra"].into_iter().enumerate() {
            let request = effort_probe(
                &format!("effort-ceiling-{index}"),
                Some(effort::Level::High),
                4096,
                Some(effort_ceiling_access(true, ceiling)),
            );
            assert_eq!(
                effort_reached_unary(&request).await,
                [(Some(effort::Level::High), 4096)],
                "ceiling {ceiling:?} must not lower the request"
            );
        }
        // A ceiling above the request leaves the request where it was.
        let below = effort_probe(
            "effort-below-ceiling",
            Some(effort::Level::Low),
            0,
            Some(effort_ceiling_access(true, "max")),
        );
        assert_eq!(
            effort_reached_unary(&below).await,
            [(Some(effort::Level::Low), 0)]
        );
    }

    #[test]
    fn an_unpinned_anthropic_request_is_bounded_by_the_ceiling_after_its_model_resolves() {
        // An unpinned request names no model, so apply_effort_ceiling sees no Claude
        // 5.5 default; request_for then rewrites it to the Anthropic default. The
        // ceiling must bound the resolved model's default as well, or an org capped
        // at low would run the common chat path at the model's default.
        let chain = chain_with(Arc::new(Mutex::new(None)));
        let unpinned = InferRequest {
            request_id: "effort-unpinned".to_owned(),
            model: String::new(),
            access: Some(effort_ceiling_access(true, "low")),
            ..Default::default()
        };
        let sent = chain.request_for(&unpinned, &unpinned.model, "anthropic");
        assert_eq!(sent.model, "claude-sonnet-5-5");
        assert_eq!(sent.reasoning_effort, Some(effort::Level::Low));

        // No ceiling, or a shadow one, leaves the level to the model default.
        let free = InferRequest { access: None, ..unpinned.clone() };
        assert_eq!(chain.request_for(&free, &free.model, "anthropic").reasoning_effort, None);
        let shadow = InferRequest {
            access: Some(effort_ceiling_access(false, "low")),
            ..unpinned.clone()
        };
        assert_eq!(chain.request_for(&shadow, &shadow.model, "anthropic").reasoning_effort, None);
    }

    #[test]
    fn an_effort_ceiling_bounds_the_claude_5_5_default_of_a_request_naming_no_level() {
        // A Claude 5.5 request with neither a level nor a budget runs at its model's
        // default (high on Sonnet 5.5), which the adapter sends explicitly. The
        // ceiling bounds that level too, so a thread saved on a retired id cannot run
        // above the org's cap just because it names no effort.
        let sonnet = InferRequest {
            request_id: "effort-5-5-default".to_owned(),
            model: "claude-sonnet-5-5".to_owned(),
            access: Some(effort_ceiling_access(true, "low")),
            ..Default::default()
        };
        let bounded = FallbackChain::apply_effort_ceiling(&sonnet)
            .expect("an enforced low ceiling lowers the high default of claude-sonnet-5-5");
        assert_eq!(bounded.reasoning_effort, Some(effort::Level::Low));
        assert_eq!(bounded.thinking_budget_tokens, 0);

        // Without an enforced ceiling the request is passed through unchanged.
        let unceiled = InferRequest {
            access: None,
            ..sonnet.clone()
        };
        assert!(FallbackChain::apply_effort_ceiling(&unceiled).is_none());
        let shadow = InferRequest {
            access: Some(effort_ceiling_access(false, "low")),
            ..sonnet.clone()
        };
        assert!(FallbackChain::apply_effort_ceiling(&shadow).is_none());

        // A ceiling that leaves the default alone changes nothing, and so does a
        // model with no 5.5 default.
        let at_default = InferRequest {
            access: Some(effort_ceiling_access(true, "high")),
            ..sonnet.clone()
        };
        assert!(FallbackChain::apply_effort_ceiling(&at_default).is_none());
        let earlier = InferRequest {
            model: "claude-sonnet-4-6".to_owned(),
            ..sonnet.clone()
        };
        assert!(FallbackChain::apply_effort_ceiling(&earlier).is_none());

        // Opus 5.5 defaults to medium, and a negative budget asks for no thinking
        // just as zero does, so the ceiling bounds the default there too.
        let opus = InferRequest {
            model: "claude-opus-5-5".to_owned(),
            ..sonnet.clone()
        };
        let opus_bounded =
            FallbackChain::apply_effort_ceiling(&opus).expect("medium is above a low ceiling");
        assert_eq!(opus_bounded.reasoning_effort, Some(effort::Level::Low));
        let negative = InferRequest {
            thinking_budget_tokens: -1,
            ..sonnet
        };
        let negative_bounded = FallbackChain::apply_effort_ceiling(&negative)
            .expect("a negative budget leaves the default to the ceiling");
        assert_eq!(negative_bounded.reasoning_effort, Some(effort::Level::Low));
        assert_eq!(negative_bounded.thinking_budget_tokens, -1);
    }

    #[test]
    fn the_ladder_is_pruned_to_deployments_that_exist_and_stays_bounded() {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let mut chain = ladder_chain(&asked, vec![], vec![]);

        // Unknown catalogs prune nothing, but the step bound still holds.
        let unpruned = chain.tool_ladder("claude-sonnet-4-6", None, "", None);
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
            chain.tool_ladder("claude-sonnet-4-6", None, "", None),
            ["claude-haiku-4-5", "gpt-4o-mini"]
        );
    }

    #[test]
    fn the_ladder_drops_the_rungs_the_organisation_hides() {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let mut chain = ladder_chain(&asked, vec![], vec![]);
        chain.deployed = DeployedModels {
            anthropic: Some(vec![
                "claude-sonnet-4-6".to_owned(),
                "claude-haiku-4-5".to_owned(),
            ]),
            openai: Some(vec!["gpt-4o-mini".to_owned()]),
        };
        let org = OrgOverlay {
            hidden: vec!["claude-haiku-4-5".to_owned()],
            ..OrgOverlay::default()
        };
        assert_eq!(
            chain.tool_ladder("claude-sonnet-4-6", None, "", Some(&org)),
            ["gpt-4o-mini"]
        );
    }

    #[test]
    fn the_org_overlay_hides_by_id_or_successor_not_by_an_unspecified_id() {
        let org = OrgOverlay {
            hidden: vec!["gpt-4o-mini".to_owned()],
            ..OrgOverlay::default()
        };
        assert!(org_hides(Some(&org), "gpt-4o-mini"));
        assert!(org_hides(Some(&org), "GPT-4o-Mini"));
        assert!(!org_hides(Some(&org), "gpt-4o"));
        // An automatic call names no model: the walk judges the provider default it resolves to instead.
        assert!(!org_hides(Some(&org), ""));
        assert!(!org_hides(None, "gpt-4o-mini"));

        // A retired Claude id is served as its successor, so hiding the successor hides the retired id too.
        let successor = OrgOverlay {
            hidden: vec!["claude-sonnet-5-5".to_owned()],
            ..OrgOverlay::default()
        };
        assert!(org_hides(Some(&successor), "claude-sonnet-4-20250514"));
    }

    #[test]
    fn the_organisation_overlay_refuses_a_role_candidate_it_hides() {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let chain = ladder_chain(&asked, vec![], vec![]);
        let req = route_request("role-org", "");
        let catalogue = [ModelInfo {
            id: "gpt-4o-mini".to_owned(),
            ..ModelInfo::default()
        }];
        let org = OrgOverlay {
            hidden: vec!["gpt-4o-mini".to_owned()],
            ..OrgOverlay::default()
        };
        assert!(chain.role_candidate_admitted(&req, &catalogue, None, "gpt-4o-mini"));
        assert!(!chain.role_candidate_admitted(&req, &catalogue, Some(&org), "gpt-4o-mini"));
    }

    /// The intent layer judges its pick against the organisation's overlay as the walk does (V3-inf-1): a pick the
    /// organisation hides gives way to the next visible rung, then to the cheap fallback when the ladder has none, and
    /// stays when nothing is visible, so the walk refuses it as hidden. An organisation that hides nothing keeps it.
    #[tokio::test]
    async fn an_intent_pick_the_organisation_hides_gives_way_to_the_next_visible_candidate() {
        use crate::provider::routing_policy::ComplexityModels;
        let asked = Arc::new(Mutex::new(Vec::new()));
        let chain = ladder_chain(&asked, vec![], vec![]);
        let mut policy = RoutingPolicy::default();
        policy.table.balance = ComplexityModels {
            simple: "gpt-5-mini".to_owned(),
            moderate: "gpt-5-mini".to_owned(),
            complex: "gpt-5-mini".to_owned(),
        };
        policy.tool_fallback_ladder = vec!["gpt-4o-mini".to_owned(), "gpt-5-nano".to_owned()];
        chain.policy_handle().store(Arc::new(policy.clone()));
        let req = route_request("org-pick", "verevon-balance");

        let hides_pick = OrgOverlay {
            hidden: vec!["gpt-5-mini".to_owned()],
            ..OrgOverlay::default()
        };
        let (moved, _) = chain
            .resolve_intent(&req, Some(&hides_pick))
            .await
            .expect("the intent layer is on");
        assert_eq!(moved.model, "gpt-4o-mini", "the hidden pick was kept");
        let (kept, _) = chain
            .resolve_intent(&req, None)
            .await
            .expect("the intent layer is on");
        assert_eq!(
            kept.model, "gpt-5-mini",
            "an organisation that hides nothing moved the pick"
        );

        // A ladderless policy leaves the cheap fallback as the only candidate.
        policy.tool_fallback_ladder.clear();
        chain.policy_handle().store(Arc::new(policy.clone()));
        let (cheap, _) = chain
            .resolve_intent(&req, Some(&hides_pick))
            .await
            .expect("the intent layer is on");
        assert_eq!(cheap.model, policy.cheap_fallback);

        // With every candidate hidden the pick stays, and the walk refuses it.
        let hides_every_candidate = OrgOverlay {
            hidden: vec!["gpt-5-mini".to_owned(), policy.cheap_fallback.clone()],
            ..OrgOverlay::default()
        };
        let (stays, _) = chain
            .resolve_intent(&req, Some(&hides_every_candidate))
            .await
            .expect("the intent layer is on");
        assert_eq!(stays.model, "gpt-5-mini");
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
            codex_subscription: String::new(),
        };
        let req = InferRequest {
            thinking_budget_tokens: 0,
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

    fn image_request(model: &str) -> InferRequest {
        InferRequest {
            request_id: "img-1".to_owned(),
            model: model.to_owned(),
            messages: vec![crate::provider::ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "what is this".to_owned(),
                name: String::new(),
            }],
            image_parts: vec![crate::provider::ImagePart {
                message_index: 0,
                data: Arc::from(&b"png"[..]),
                mime_type: "image/png".to_owned(),
            }],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn image_request_skips_non_vision_provider() {
        let blind_seen = Arc::new(Mutex::new(None));
        let sighted_seen = Arc::new(Mutex::new(None));
        let blind: BoxedProvider =
            Arc::new(RecordingProvider { seen_model: blind_seen.clone(), ..Default::default() });
        let sighted: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: sighted_seen.clone(),
            vision: true,
            ..Default::default()
        });
        let chain = FallbackChain::new_with_providers(
            vec![("blind".to_owned(), blind), ("sighted".to_owned(), sighted)],
            1,
        );

        let response = chain.infer(&image_request("gpt-4o")).await.unwrap();
        assert_eq!(response.model_used, "gpt-4o");
        assert!(blind_seen.lock().unwrap().is_none(), "the provider that cannot read images is never called");
        assert_eq!(sighted_seen.lock().unwrap().as_deref(), Some("gpt-4o"));

        *sighted_seen.lock().unwrap() = None;
        chain.infer_stream(&image_request("gpt-4o")).await.unwrap();
        assert!(blind_seen.lock().unwrap().is_none());
        assert_eq!(sighted_seen.lock().unwrap().as_deref(), Some("gpt-4o"));
    }

    #[tokio::test]
    async fn image_request_with_no_vision_provider_fails_closed_typed() {
        let seen = Arc::new(Mutex::new(None));
        let blind: BoxedProvider =
            Arc::new(RecordingProvider { seen_model: seen.clone(), ..Default::default() });
        let denylisted: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen.clone(),
            vision: true,
            ..Default::default()
        });
        let chain = FallbackChain::new_with_providers(vec![("blind".to_owned(), blind)], 1);

        let error = chain.infer(&image_request("gpt-4o")).await.unwrap_err();
        assert!(matches!(error, ProviderError::VisionUnsupported(_)), "{error:?}");
        let error = chain.infer_stream(&image_request("gpt-4o")).await.unwrap_err();
        assert!(matches!(error, ProviderError::VisionUnsupported(_)), "{error:?}");

        // A provider that reads images still refuses a model that does not.
        let chain = FallbackChain::new_with_providers(vec![("sighted".to_owned(), denylisted)], 1);
        let error = chain.infer(&image_request("deepseek-chat")).await.unwrap_err();
        assert!(matches!(error, ProviderError::VisionUnsupported(_)), "{error:?}");

        assert!(seen.lock().unwrap().is_none(), "no provider was called");
    }

    #[tokio::test]
    async fn a_text_request_does_not_meet_the_vision_gate() {
        let seen = Arc::new(Mutex::new(None));
        let blind: BoxedProvider =
            Arc::new(RecordingProvider { seen_model: seen.clone(), ..Default::default() });
        let chain = FallbackChain::new_with_providers(vec![("blind".to_owned(), blind)], 1);
        let mut text_only = image_request("gpt-4o");
        text_only.image_parts.clear();

        chain.infer(&text_only).await.unwrap();
        assert_eq!(seen.lock().unwrap().as_deref(), Some("gpt-4o"));
    }

    /// A routing double for the same-tier anchor and the user-route guard: it declares its own id,
    /// family and residency, counts its calls, and either serves or fails them.
    struct RouteStub {
        provider_id: &'static str,
        family: ModelFamily,
        residency: Residency,
        fails: bool,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl RouteStub {
        fn boxed(
            provider_id: &'static str,
            family: ModelFamily,
            residency: Residency,
            fails: bool,
        ) -> (BoxedProvider, Arc<std::sync::atomic::AtomicUsize>) {
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let provider: BoxedProvider = Arc::new(Self {
                provider_id,
                family,
                residency,
                fails,
                calls: calls.clone(),
            });
            (provider, calls)
        }
    }

    #[async_trait::async_trait]
    impl ProviderRouter for RouteStub {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities {
                provider_id: self.provider_id.to_owned(),
                model_family: self.family,
                residency: self.residency,
                ..crate::provider::ProviderCapabilities::default()
            }
        }

        async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fails {
                return Err(ProviderError::Http("500 Internal Server Error: upstream".to_owned()));
            }
            Ok(InferResponse {
                request_id: req.request_id.clone(),
                content: "ok".to_owned(),
                model_used: req.model.clone(),
                stop_reason: "stop".to_owned(),
                ..InferResponse::default()
            })
        }

        async fn infer_stream(
            &self,
            _req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ProviderError::Unavailable("route stub".to_owned()))
        }
    }

    fn route_request(id: &str, model: &str) -> InferRequest {
        InferRequest {
            request_id: id.to_owned(),
            model: model.to_owned(),
            ..Default::default()
        }
    }

    /// Both switches off: a failed EU first hop is followed by the GLOBAL provider, as before.
    #[tokio::test]
    async fn route_rules_off_walk_is_unchanged() {
        use std::sync::atomic::Ordering;
        let (eu, eu_calls) =
            RouteStub::boxed("azure-openai", ModelFamily::OpenAiCompatible, Residency::Eu, true);
        let (global, global_calls) =
            RouteStub::boxed("openai", ModelFamily::OpenAiCompatible, Residency::Global, false);
        let chain = FallbackChain::new_with_providers(
            vec![("azure-openai".to_owned(), eu), ("openai".to_owned(), global)],
            1,
        )
        .with_benching(false)
        .with_same_tier_fallback(false);

        chain.infer(&route_request("off-1", "gpt-4o-mini")).await.expect("GLOBAL serves");
        assert_eq!(eu_calls.load(Ordering::SeqCst), 1);
        assert_eq!(global_calls.load(Ordering::SeqCst), 1);
    }

    /// Same-tier on: a GLOBAL provider is never tried after an EU_RESIDENT first hop failed.
    #[tokio::test]
    async fn same_tier_never_falls_to_a_lower_tier_within_one_rung() {
        use std::sync::atomic::Ordering;
        let (eu, eu_calls) =
            RouteStub::boxed("azure-openai", ModelFamily::OpenAiCompatible, Residency::Eu, true);
        let (global, global_calls) =
            RouteStub::boxed("openai", ModelFamily::OpenAiCompatible, Residency::Global, false);
        let chain = FallbackChain::new_with_providers(
            vec![("azure-openai".to_owned(), eu), ("openai".to_owned(), global)],
            1,
        )
        .with_same_tier_fallback(true);

        let outcome = chain.infer(&route_request("tier-1", "gpt-4o-mini")).await;
        // A failed eligible provider is exhaustion, not a tier refusal, and nothing is downgraded.
        assert!(outcome.is_err(), "the only provider at the anchor tier failed");
        assert_eq!(eu_calls.load(Ordering::SeqCst), 1);
        assert_eq!(global_calls.load(Ordering::SeqCst), 0, "GLOBAL is never tried");
    }

    /// The anchor holds on a later rung of the ladder and across the pass reset: the first model fails
    /// on the EU provider, the second model is served only by a GLOBAL provider, and the call never
    /// reaches it. With the switch off the same walk does reach it.
    #[tokio::test]
    async fn same_tier_anchor_holds_on_a_later_rung_and_across_the_pass_reset() {
        use std::sync::atomic::Ordering;
        for same_tier in [true, false] {
            let (eu, eu_calls) = RouteStub::boxed(
                "azure-openai",
                ModelFamily::OpenAiCompatible,
                Residency::Eu,
                true,
            );
            let (global, global_calls) =
                RouteStub::boxed("anthropic", ModelFamily::Anthropic, Residency::Global, false);
            let chain = FallbackChain::new_with_providers(
                vec![("azure-openai".to_owned(), eu), ("anthropic".to_owned(), global)],
                1,
            )
            .with_same_tier_fallback(same_tier);
            let req = route_request("tier-2", "gpt-4o-mini");
            let mut attempts = 0u32;
            let mut throttle = ThrottleState::default();
            let mut clock = CallClock::start();

            let first = chain
                .infer_one_model(
                    &req,
                    "gpt-4o-mini",
                    &mut attempts,
                    &mut throttle,
                    &mut clock,
                    None,
                )
                .await;
            assert!(first.is_none(), "the EU provider fails the first rung");
            throttle.next_pass();
            let second = chain
                .infer_one_model(
                    &req,
                    "claude-sonnet-4",
                    &mut attempts,
                    &mut throttle,
                    &mut clock,
                    None,
                )
                .await;

            assert_eq!(eu_calls.load(Ordering::SeqCst), 1);
            if same_tier {
                assert_eq!(throttle.anchor_tier, Some(PrivacyTier::EuResident));
                assert!(second.is_none(), "the GLOBAL provider is below the anchor");
                assert_eq!(global_calls.load(Ordering::SeqCst), 0);
            } else {
                assert_eq!(throttle.anchor_tier, None);
                assert!(second.is_some(), "without the anchor the later rung is served");
                assert_eq!(global_calls.load(Ordering::SeqCst), 1);
            }
        }
    }

    /// Benching on: a request with no hint and no connection never reaches the subscription provider
    /// and makes no attempt on it; the platform provider after it serves.
    #[tokio::test]
    async fn benching_keeps_an_unhinted_request_off_the_subscription_route() {
        use std::sync::atomic::Ordering;
        let id = mp_contracts::agentic_subscription::PROVIDER_ID;
        let (subscription, subscription_calls) =
            RouteStub::boxed(id, ModelFamily::OpenAiCompatible, Residency::Eu, false);
        let (platform, platform_calls) =
            RouteStub::boxed("azure-openai", ModelFamily::OpenAiCompatible, Residency::Eu, false);
        let providers = || {
            vec![
                (id.to_owned(), subscription.clone()),
                ("azure-openai".to_owned(), platform.clone()),
            ]
        };

        let guarded = FallbackChain::new_with_providers(providers(), 1).with_benching(true);
        guarded.infer(&route_request("bench-1", "gpt-4o-mini")).await.expect("platform serves");
        assert_eq!(subscription_calls.load(Ordering::SeqCst), 0, "no attempt on the subscription");
        assert_eq!(platform_calls.load(Ordering::SeqCst), 1);

        // Off, the walk is byte-identical to today: the subscription provider is asked first.
        let unguarded = FallbackChain::new_with_providers(providers(), 1);
        unguarded.infer(&route_request("bench-2", "gpt-4o-mini")).await.expect("served");
        assert_eq!(subscription_calls.load(Ordering::SeqCst), 1);
    }

    /// Benching on: a request that names the subscription provider and carries a connection still
    /// reaches it; naming it without a connection does not, and moves nowhere else.
    #[tokio::test]
    async fn benching_lets_a_hinted_subscription_request_with_a_connection_through() {
        use std::sync::atomic::Ordering;
        let id = mp_contracts::agentic_subscription::PROVIDER_ID;
        let (subscription, subscription_calls) =
            RouteStub::boxed(id, ModelFamily::OpenAiCompatible, Residency::Eu, false);
        let (platform, platform_calls) =
            RouteStub::boxed("azure-openai", ModelFamily::OpenAiCompatible, Residency::Eu, false);
        let chain = FallbackChain::new_with_providers(
            vec![(id.to_owned(), subscription), ("azure-openai".to_owned(), platform)],
            1,
        )
        .with_benching(true);

        let mut pinned = route_request("bench-3", "gpt-4o-mini");
        pinned.provider_hint = id.to_owned();
        pinned.subscription_connection_id = "connection".to_owned();
        chain.infer(&pinned).await.expect("the pinned subscription request is served");
        assert_eq!(subscription_calls.load(Ordering::SeqCst), 1);

        let mut no_connection = route_request("bench-4", "gpt-4o-mini");
        no_connection.provider_hint = id.to_owned();
        assert!(chain.infer(&no_connection).await.is_err());
        assert_eq!(subscription_calls.load(Ordering::SeqCst), 1, "not asked without a connection");
        assert_eq!(platform_calls.load(Ordering::SeqCst), 0, "never moved onto a platform route");
    }
}

/// Chain benching under `INFERENCE_PROVIDER_BENCHING` (R2-OD-40-b3).
#[cfg(test)]
#[path = "fallback_bench_tests.rs"]
mod bench_tests;
