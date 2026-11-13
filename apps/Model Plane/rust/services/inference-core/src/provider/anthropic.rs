//! Anthropic Claude provider.
//!
//! Two flavors share one Anthropic Messages request/response codec:
//! * **Direct** — `https://api.anthropic.com/v1/messages` (first-party).
//! * **Azure Foundry** — the Claude deployments on an Azure AI Foundry
//!   resource, served at `https://<resource>.services.ai.azure.com/anthropic/v1/messages`.
//!   Verified live: the body is the *native* Anthropic Messages shape and auth
//!   is the **same `x-api-key` header** as the direct API (the `api-key` header
//!   and an `api-version` query param both 401 — do not add them). This is the
//!   fix for the direct API returning 400 "credit balance too low".

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use mp_contracts::prompt_tier::PromptTier;

use super::effort::{self, Family, Wire};
use super::model_record;
use super::zdr::ZdrAttestation;
use super::{InferChunk, InferRequest, InferResponse, ModelInfo, ProviderError, ProviderRouter};

#[cfg(test)]
#[path = "anthropic_compaction_tests.rs"]
mod checkpoint_tests;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
// Stays on 2023-06-01. This adapter never sends temperature, top_p or top_k, which
// API version 2026-10-21 refuses on every model, so the newer version would remove
// nothing from what is sent here. Moving the header is a separate upgrade.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The Claude 5.5 models: the ids the catalogue offers, and the ids the wire rules
/// below are written against. Exact strings, never a date-stamped form.
const CLAUDE_OPUS_5_5: &str = "claude-opus-5-5";
const CLAUDE_SONNET_5_5: &str = "claude-sonnet-5-5";
const CLAUDE_HAIKU_5_5: &str = "claude-haiku-5-5";

/// Default Claude model used when a request leaves the model unspecified
/// ("Verevon Auto"). The fallback chain substitutes this when Anthropic is the
/// provider serving an unpinned request, so chat works against an
/// Anthropic-only deployment with no client- or operator-chosen model.
pub(crate) const DEFAULT_ANTHROPIC_MODEL: &str = CLAUDE_SONNET_5_5;

/// Default Claude deployment for the Azure Foundry flavor when a request leaves
/// the model unspecified. Names the cheapest chat-capable Claude deployment so
/// an unpinned Claude request stays economical.
pub(crate) const DEFAULT_AZURE_ANTHROPIC_MODEL: &str = "claude-haiku-4-5";

/// Which Anthropic Messages endpoint this provider targets.
#[derive(Clone)]
enum AnthropicFlavor {
    /// First-party `api.anthropic.com`.
    Direct,
    /// Azure AI Foundry resource. `endpoint` is the resource base
    /// (e.g. `https://<resource>.services.ai.azure.com`); the Messages route
    /// `/anthropic/v1/messages` is appended. `models` is the deployed Claude
    /// catalog used for `list_models`.
    Azure {
        endpoint: String,
        models: Vec<String>,
    },
}

/// Anthropic Claude inference provider.
#[derive(Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    flavor: AnthropicFlavor,
    /// Evidence-bound ZDR attestation for this exact resource, or `None` when the
    /// operator makes no ZDR claim.
    ///
    /// Before this field existed, `capabilities()` hardcoded `supports_zdr: false`
    /// with no way to override it, so *every* ZDR-flagged request was skipped past
    /// both Claude routes — direct and Azure Foundry — and a chain with no other
    /// ZDR-capable provider returned `ZdrUnavailable`. The exclusion was silent:
    /// nothing distinguished "Claude cannot do ZDR" from "Claude was not asked".
    zdr: Option<Arc<ZdrAttestation>>,
    /// The strongest residency guarantee this resource honors.
    residency: super::Residency,
}

impl AnthropicProvider {
    /// Create a new direct (`api.anthropic.com`) Anthropic provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is empty.
    pub fn new(api_key: impl Into<String>) -> Result<Self, ProviderError> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(ProviderError::Unavailable(
                "ANTHROPIC_API_KEY is empty".to_owned(),
            ));
        }

        Ok(Self {
            client: crate::provider::provider_http_client(),
            api_key,
            flavor: AnthropicFlavor::Direct,
            zdr: None,
            residency: super::Residency::Global,
        })
    }

    /// Create an Azure AI Foundry Anthropic provider.
    ///
    /// `endpoint` is the resource base (the `services.ai.azure.com` host —
    /// `cognitiveservices.azure.com` 401s for the Anthropic route). The Claude
    /// Messages route and `x-api-key` auth are appended at call time.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::Unavailable`] if `api_key` or `endpoint` is empty.
    pub fn new_azure(
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
        models: Vec<String>,
    ) -> Result<Self, ProviderError> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(ProviderError::Unavailable(
                "AZURE_ANTHROPIC_API_KEY is empty".to_owned(),
            ));
        }
        let endpoint = endpoint.into();
        if endpoint.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "AZURE_ANTHROPIC_ENDPOINT is empty".to_owned(),
            ));
        }
        Ok(Self {
            client: crate::provider::provider_http_client(),
            api_key,
            flavor: AnthropicFlavor::Azure { endpoint, models },
            zdr: None,
            residency: super::Residency::Global,
        })
    }

    /// Attach an evidence-bound ZDR attestation to this exact resource.
    ///
    /// Mirrors [`super::openai::OpenAiProvider::with_zdr_attestation`]. The
    /// direct `api.anthropic.com` route is never promoted implicitly: a caller
    /// must pass an attestation that named that surface.
    #[must_use]
    pub fn with_zdr_attestation(mut self, attestation: Option<Arc<ZdrAttestation>>) -> Self {
        self.zdr = attestation;
        self
    }

    /// Declare the strongest residency guarantee this resource honors.
    #[must_use]
    pub const fn with_residency(mut self, residency: super::Residency) -> Self {
        self.residency = residency;
        self
    }

    /// The Messages API URL for the active flavor.
    fn messages_url(&self) -> String {
        match &self.flavor {
            AnthropicFlavor::Direct => ANTHROPIC_API_URL.to_owned(),
            AnthropicFlavor::Azure { endpoint, .. } => {
                format!("{}/anthropic/v1/messages", endpoint.trim_end_matches('/'))
            }
        }
    }

    /// Provider name reported in logs and `list_models`.
    fn provider_name(&self) -> &'static str {
        match &self.flavor {
            AnthropicFlavor::Direct => "anthropic",
            AnthropicFlavor::Azure { .. } => "azure-anthropic",
        }
    }
}

/// True for the economy Claude tier (Haiku) so the UI can group cheap models.
fn is_cheap_claude(model: &str) -> bool {
    model.to_ascii_lowercase().contains("haiku")
}

/// Anthropic's hard limit on `cache_control` breakpoints in a single request. A
/// fifth breakpoint is a `400`, which would fail a user's whole turn — whereas a
/// breakpoint we decline to place merely costs a few tokens. The planner below
/// places at most three (tools, system, last message) and still checks against
/// this ceiling, so the invariant is enforced in code and not only by
/// construction; the spare slot is headroom for an account-side automatic cache
/// breakpoint, which consumes one of the same four slots.
const MAX_CACHE_BREAKPOINTS: usize = 4;

/// The `cache_control` marker for Anthropic's default 5-minute ephemeral prompt
/// cache.
///
/// **No `anthropic-beta` header is required.** Prompt caching is generally
/// available on `anthropic-version: 2023-06-01` — the header this provider
/// already sends — and the `prompt-caching-2024-07-31` beta header was only
/// needed during the 2024 beta. Only the *1-hour* extended TTL
/// (`{"type":"ephemeral","ttl":"1h"}`) and the cache-diagnostics beta need an
/// extra header, and this provider uses neither: it sends the plain 5-minute
/// `ephemeral` type, which is also what the Azure AI Foundry Claude deployments
/// accept (Foundry lists prompt caching as a supported Claude capability and
/// meters "tokens after the last cache breakpoint" for its rate limits).
fn ephemeral_cache_control() -> serde_json::Value {
    serde_json::json!({ "type": "ephemeral" })
}

/// Minimum cacheable prompt length, in tokens, for `model`.
///
/// Anthropic refuses to cache a prefix shorter than this even when it *is*
/// marked — the breakpoint is silently ignored rather than rejected — so an
/// over-high estimate costs a missed cache while an over-low one wastes a
/// breakpoint slot. The published minimums differ per model family and apply
/// identically on the first-party Claude API and on Microsoft Foundry. Unknown
/// or future deployment names fall back to the Sonnet-class 1024.
fn min_cacheable_tokens(model: &str) -> usize {
    let model = model.to_ascii_lowercase();
    // Each generation publishes its own floor, and the floors are not monotonic:
    // 512 on Opus 5 and later, Sonnet 5.5 and Haiku 5.5; 1024 on Sonnet 5; 4096 on
    // Opus 4.5 / 4.6 and Haiku 4.5. Rows are checked in order, so a narrower name
    // precedes the family it sits inside.
    if model.contains("haiku-5") {
        512
    } else if model.contains("haiku-4") {
        4_096
    } else if model.contains("haiku") {
        // Haiku 3.5, and any Haiku name this table does not know.
        2_048
    } else if model.contains("opus-4-5")
        || model.contains("opus-4.5")
        || model.contains("opus-4-6")
        || model.contains("opus-4.6")
    {
        4_096
    } else if model.contains("opus-4-7")
        || model.contains("opus-4.7")
        || model.contains("mythos-preview")
    {
        2_048
    } else if model.contains("opus-5")
        || model.contains("sonnet-5-5")
        || model.contains("fable-5")
        || model.contains("mythos-5")
    {
        512
    } else {
        // Sonnet-class (Sonnet 4.x, Sonnet 5, Opus 4.8). Unknown and future names
        // land here too: the conservative direction for a generation not in the table.
        1_024
    }
}

/// Conservative token estimate for `bytes` of prompt text.
///
/// inference-core has no real tokenizer yet (quality plan §2.2), so this reuses
/// the plane's `bytes / 4` heuristic. The error direction is deliberate: real
/// tokenizers emit *more* tokens than `bytes / 4` for both JSON tool schemas
/// (punctuation-dense) and Norwegian prose (`æ/ø/å` split aggressively), so a
/// prefix this function judges long enough is long enough in practice. When it
/// is wrong it under-counts, which makes us decline a breakpoint rather than
/// place a useless one.
const fn estimated_tokens(bytes: usize) -> usize {
    bytes / 4
}

/// Switch for sending the system prompt as ordered blocks, one per prompt tier,
/// with the Stable and Context blocks marked for caching (build plan WS02-S9).
/// Off, the system prompt is the single joined string it always was. Only the
/// literal `true` enables it, so a typo can never change the wire shape.
const TIERED_SYSTEM_ENV: &str = "INFERENCE_ANTHROPIC_TIERED_SYSTEM";

fn tiered_system_enabled() -> bool {
    tiered_system_flag_enabled(std::env::var(TIERED_SYSTEM_ENV).ok().as_deref())
}

/// Pure parser behind [`tiered_system_enabled`], for the same reason as
/// [`context_management_flag_enabled`].
fn tiered_system_flag_enabled(raw: Option<&str>) -> bool {
    matches!(raw, Some("true"))
}

/// The system text of a request whose caller named a tier plan: one block per
/// non-empty tier, in prompt order (Stable, Context, then the per-turn
/// remainder as Volatile). `None` when the plan does not describe the request,
/// so a stale or wrong plan degrades to the single joined string instead of
/// marking text that changes every turn.
///
/// The plan counts messages from index 0, so it only holds when every counted
/// message is a system message; the remainder is every later system message in
/// order, which is what the joined string held.
fn tiered_system_blocks(req: &InferRequest) -> Option<Vec<(PromptTier, String)>> {
    let plan = req.prompt_tiers;
    let counted = plan.stable.checked_add(plan.context)?;
    if counted == 0 || counted > req.messages.len() {
        return None;
    }
    if !req.messages[..counted]
        .iter()
        .all(|m| m.role.eq_ignore_ascii_case("system"))
    {
        return None;
    }
    let join = |messages: &[super::ChatMessage]| -> String {
        messages
            .iter()
            .filter(|m| m.role.eq_ignore_ascii_case("system"))
            .map(|m| m.content.trim())
            .filter(|content| !content.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let (stable, rest) = req.messages.split_at(plan.stable);
    let (context, rest) = rest.split_at(plan.context);
    let blocks: Vec<(PromptTier, String)> = [
        (PromptTier::Stable, join(stable)),
        (PromptTier::Context, join(context)),
        (PromptTier::Volatile, join(rest)),
    ]
    .into_iter()
    .filter(|(_, text)| !text.is_empty())
    .collect();
    (!blocks.is_empty()).then_some(blocks)
}

/// Mark the stable prefix of an Anthropic request with prompt-cache
/// breakpoints.
///
/// # Why this exists
///
/// model-gateway's inline chat loop runs up to 12 tool rounds per turn, and
/// every round re-sends the *entire* system prompt, all tool definitions, and
/// the whole accumulated history at full input-token price. Anthropic caches
/// everything **up to and including** a block marked `cache_control`, over the
/// hierarchy `tools` → `system` → `messages` (in that order), so *where* a
/// breakpoint sits — not how many there are — decides how much of each round is
/// billed at the ~10% cache-read rate instead of 100%.
///
/// # Placement
///
/// * **Last tool definition.** The tool block is byte-identical across every
///   round of a turn and sits first in the hierarchy, making it the single most
///   reusable prefix.
/// * **The system prompt.** Also byte-identical across rounds, and its prefix
///   subsumes `tools`, so this one breakpoint caches the tool block too.
/// * **The last message.** The rolling breakpoint that makes the *loop* cheap:
///   round N writes an entry at its final message, and round N+1's breakpoint
///   walks backwards to find it (the lookback spans 20 blocks and a round
///   appends only ~2), so round N+1 pays the cache-write rate on the delta
///   alone.
///
/// Anthropic's minimum applies to the whole cacheable *prefix*, not to the
/// marked block on its own, so the byte counter below accumulates in hierarchy
/// order. A block that is skipped for being too small still contributes its
/// bytes to the next candidate — and is still cached by it, since a later
/// breakpoint's prefix contains it.
///
/// # ⚠ ZERO DATA RETENTION
///
/// Prompt caching means the **provider retains the prompt prefix server-side**
/// for minutes. That is data retention, so it is disabled outright whenever
/// `req.zdr` is set — the same discipline [`crate::cache`] applies when it
/// bypasses the local prompt cache on both read *and* write. Under ZDR this
/// returns with the body untouched, so no `cache_control` reaches the wire from
/// any path: `infer` and `infer_stream` both build their body here, and this is
/// the only place in the provider that emits the marker.
fn apply_prompt_caching(body: &mut serde_json::Value, req: &InferRequest) {
    // ⚠ COMPLIANCE GATE — see the "ZERO DATA RETENTION" note above. This must
    // stay the first statement in this function.
    if req.zdr {
        return;
    }

    let min_tokens = min_cacheable_tokens(&req.model);
    let mut breakpoints = 0_usize;
    // Bytes of prompt content at or before the block under consideration.
    let mut prefix_bytes = 0_usize;

    // 1. Tool definitions — first in the cache hierarchy, byte-identical across
    //    every round of a tool loop. `cache_control` hangs off the LAST tool,
    //    which marks the whole `tools` block.
    prefix_bytes += req
        .tools
        .iter()
        .map(|t| t.name.len() + t.description.len() + t.parameters_json.len())
        .sum::<usize>();
    if breakpoints < MAX_CACHE_BREAKPOINTS && estimated_tokens(prefix_bytes) >= min_tokens {
        if let Some(last_tool) = body
            .get_mut("tools")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|tools| tools.last_mut())
        {
            last_tool["cache_control"] = ephemeral_cache_control();
            breakpoints += 1;
        }
    }

    // 2. System prompt — byte-identical across rounds; its prefix subsumes the
    //    tool block. A marked system prompt must use the structured block form,
    //    since the plain-string form has nowhere to hang `cache_control`.
    //
    //    A tiered request already is in block form, one block per tier: the
    //    Stable and Context blocks are marked (when the prefix so far is long
    //    enough), the per-turn remainder never is. That is at most two
    //    breakpoints here, so tools (1) + system (2) + last message (1) stay
    //    inside the limit of four.
    let tiered = body
        .get("system")
        .is_some_and(serde_json::Value::is_array)
        .then(|| tiered_system_blocks(req))
        .flatten();
    if let Some(blocks) = tiered {
        if let Some(system) = body
            .get_mut("system")
            .and_then(serde_json::Value::as_array_mut)
        {
            for (block, (tier, text)) in system.iter_mut().zip(&blocks) {
                prefix_bytes += text.len();
                if *tier != PromptTier::Volatile
                    && breakpoints < MAX_CACHE_BREAKPOINTS
                    && estimated_tokens(prefix_bytes) >= min_tokens
                {
                    block["cache_control"] = ephemeral_cache_control();
                    breakpoints += 1;
                }
            }
        }
    } else if let Some(system) = body
        .get("system")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
    {
        prefix_bytes += system.len();
        if !system.is_empty()
            && breakpoints < MAX_CACHE_BREAKPOINTS
            && estimated_tokens(prefix_bytes) >= min_tokens
        {
            body["system"] = serde_json::json!([{
                "type": "text",
                "text": system,
                "cache_control": ephemeral_cache_control(),
            }]);
            breakpoints += 1;
        }
    }

    // 3. The last message — the rolling breakpoint that makes a multi-round tool
    //    loop cheap (see the doc comment).
    prefix_bytes += req
        .messages
        .iter()
        .filter(|m| !m.role.eq_ignore_ascii_case("system"))
        .map(|m| m.role.len() + m.content.len())
        .sum::<usize>();
    if breakpoints < MAX_CACHE_BREAKPOINTS && estimated_tokens(prefix_bytes) >= min_tokens {
        if let Some(last_message) = body
            .get_mut("messages")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|messages| messages.last_mut())
        {
            let text = last_message
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            // Empty text blocks cannot be cached; marking one burns a slot.
            if !text.is_empty() {
                last_message["content"] = serde_json::json!([{
                    "type": "text",
                    "text": text,
                    "cache_control": ephemeral_cache_control(),
                }]);
                breakpoints += 1;
            }
        }
    }

    if breakpoints > 0 {
        debug!(
            model = %req.model,
            breakpoints,
            min_cacheable_tokens = min_tokens,
            "anthropic prompt-cache breakpoints applied"
        );
    }
}

/// Build the Anthropic messages API request body.
/// Split one Anthropic `content_block_delta` into (answer, reasoning).
///
/// Exactly one of the two is non-empty for any delta that carries readable
/// text; both are empty for deltas that carry neither (a thinking block's
/// cryptographic `signature_delta`, or a tool call's `input_json_delta`, which
/// is assembled elsewhere). Returning both channels explicitly is what keeps a
/// reasoning delta from ever being appended to the user's answer.
fn split_content_block_delta(delta: &serde_json::Value) -> (String, String) {
    // Absent `type` means a plain text delta: that is what every pre-thinking
    // stream sent, and defaulting the other way would drop all answer text.
    match delta["type"].as_str().unwrap_or("text_delta") {
        "thinking_delta" => (
            String::new(),
            delta["thinking"].as_str().unwrap_or("").to_owned(),
        ),
        "signature_delta" | "input_json_delta" => (String::new(), String::new()),
        _ => (
            delta["text"].as_str().unwrap_or("").to_owned(),
            String::new(),
        ),
    }
}

/// Anthropic's floor for `thinking.budget_tokens`. A smaller budget is a 400,
/// not a smaller amount of thinking.
const MIN_THINKING_BUDGET_TOKENS: i32 = 1024;

/// Model families that pre-date extended thinking and reject the parameter.
///
/// # Why an exclusion list rather than an allowlist
///
/// Every Claude family from 3.7 onward supports extended thinking and every
/// future one is expected to, so an allowlist would silently drop thinking for
/// each new model until someone remembered to add it — failing quietly in the
/// direction of "the feature doesn't work". An exclusion list fails in the
/// direction of "a new model gets asked to think", which is the correct default
/// for this provider and is caught immediately if wrong. The same reasoning the
/// `temperature` comment above records: what breaks on Anthropic is sending a
/// parameter the *older* models reject.
const NO_THINKING_MODEL_MARKERS: &[&str] = &[
    "claude-2",
    "claude-instant",
    "claude-3-opus",
    "claude-3-sonnet",
    "claude-3-haiku",
    "claude-3-5",
    "claude-3.5",
];

/// The `budget_tokens` to send, or `None` to omit `thinking` entirely.
///
/// Returns `None` — rather than erroring — whenever thinking cannot be honoured:
/// no budget requested, a model that rejects the parameter, a budget under
/// Anthropic's floor, or a budget that does not leave room for an answer
/// (`max_tokens` must exceed it, since the budget is spent *from* that ceiling).
/// Degrading to "no thinking" keeps a bad budget from turning a working turn
/// into a provider 400.
fn resolve_thinking_budget(model: &str, requested: i32, max_tokens: i32) -> Option<i32> {
    if requested <= 0 {
        return None;
    }
    let normalized = model.to_ascii_lowercase();
    if NO_THINKING_MODEL_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        tracing::debug!(
            model,
            "extended thinking requested for a model that rejects it; omitting"
        );
        return None;
    }
    if requested < MIN_THINKING_BUDGET_TOKENS {
        tracing::warn!(
            requested,
            minimum = MIN_THINKING_BUDGET_TOKENS,
            "extended-thinking budget below the provider minimum; omitting rather than 400ing"
        );
        return None;
    }
    // The budget is drawn from max_tokens, so an equal or larger budget leaves
    // nothing for the answer and the provider rejects it.
    if max_tokens <= requested {
        tracing::warn!(
            requested,
            max_tokens,
            "extended-thinking budget leaves no room for an answer; omitting"
        );
        return None;
    }
    Some(requested)
}

/// Switch for the ladder wire classes. Off keeps the budget-only `thinking`
/// block above byte-for-byte; on renders the canonical effort ladder per model
/// class (`effort::support`). Only the literal `true` enables it: the default
/// stays off until the live probe shows what Foundry accepts for the newest
/// models, so a typo can never switch the wire shape.
/// The Claude 5.5 models take the ladder wire whatever this says: they reject the
/// budget form, so for them the switch has no meaning (see [`is_claude_5_5`]).
const ADAPTIVE_THINKING_ENV: &str = "INFERENCE_CORE_ANTHROPIC_ADAPTIVE_THINKING";

fn adaptive_thinking_enabled() -> bool {
    adaptive_thinking_flag_enabled(std::env::var(ADAPTIVE_THINKING_ENV).ok().as_deref())
}

/// Pure parser behind [`adaptive_thinking_enabled`], for the same reason as
/// [`context_management_flag_enabled`].
fn adaptive_thinking_flag_enabled(raw: Option<&str>) -> bool {
    matches!(raw, Some("true"))
}

/// The requested level resolved against this model's wire class. The gateway
/// still speaks token budgets today, so a request with no explicit level falls
/// back to the ladder level its budget maps onto; a Claude 5.5 request with neither
/// runs at that model's documented default (see [`claude_5_5_default_effort`]).
fn resolve_ladder_effort(req: &InferRequest) -> Option<effort::Resolved> {
    let requested = req
        .reasoning_effort
        .or_else(|| effort::from_legacy_budget(req.thinking_budget_tokens))
        .or_else(|| claude_5_5_default_effort(&req.model));
    let support = effort::ModelAliases::from_env().support(Family::Anthropic, &req.model);
    effort::resolve(requested, support)
}

/// The levels the catalogue lists for a model. Empty while the ladder wire is
/// off, unless the model takes only the ladder wire (the 5.5 models): a level the
/// adapter would not render must not be offered.
fn advertised_effort_levels(model: &str, ladder: bool) -> Vec<effort::Level> {
    if !ladder && !is_claude_5_5(model) {
        return Vec::new();
    }
    effort::ModelAliases::from_env()
        .support(Family::Anthropic, model)
        .levels
        .to_vec()
}

/// The model a request names, with a Foundry deployment alias resolved to the base
/// model it stands for (see [`effort::ModelAliases`]).
fn base_model(model: &str) -> String {
    effort::ModelAliases::from_env().base_model(model)
}

/// The Claude 5.5 models. They take only the adaptive thinking wire: the legacy
/// `thinking: {type: enabled, budget_tokens}` form is rejected at every effort
/// level, so the switch that chooses the wire for other models has no say here.
fn is_claude_5_5(model: &str) -> bool {
    matches!(
        base_model(model).as_str(),
        CLAUDE_OPUS_5_5 | CLAUDE_SONNET_5_5 | CLAUDE_HAIKU_5_5
    )
}

/// The effort a Claude 5.5 request runs at when the caller names no level: the
/// model's documented default (`medium` on Opus 5.5 and Haiku 5.5, `high` on Sonnet
/// 5.5). It is sent explicitly, as the reference advises for every 5.5 model, so a
/// later change to the API default cannot move these calls without a code change.
/// The org's effort ceiling in `fallback.rs` bounds this same level.
pub(crate) fn claude_5_5_default_effort(model: &str) -> Option<effort::Level> {
    match base_model(model).as_str() {
        CLAUDE_OPUS_5_5 | CLAUDE_HAIKU_5_5 => Some(effort::Level::Medium),
        CLAUDE_SONNET_5_5 => Some(effort::Level::High),
        _ => None,
    }
}

/// Whether `model` accepts a forced `tool_choice` (`any`, or a named tool). The 5.5
/// Opus and Sonnet models answer either with a 400; Haiku 5.5 and the older models
/// the adapter serves take both.
fn accepts_forced_tool_choice(model: &str) -> bool {
    !matches!(base_model(model).as_str(), CLAUDE_OPUS_5_5 | CLAUDE_SONNET_5_5)
}

/// The `tool_choice` for the gateway's `choice` (`auto`, `none`, `required`, or a
/// tool name). Where the model refuses a forced choice it goes out as `auto`: the
/// prompt has to steer the call, and the caller checks that one was made.
fn tool_choice_wire(model: &str, choice: &str) -> serde_json::Value {
    match choice {
        "auto" => serde_json::json!({ "type": "auto" }),
        "none" => serde_json::json!({ "type": "none" }),
        _ if !accepts_forced_tool_choice(model) => {
            debug!(model, "forced tool_choice is not accepted by this model; sending auto");
            serde_json::json!({ "type": "auto" })
        }
        "required" => serde_json::json!({ "type": "any" }),
        name => serde_json::json!({ "type": "tool", "name": name }),
    }
}

/// The longest tool name the Messages API accepts.
const TOOL_NAME_MAX_LEN: usize = 128;

/// The tool a forced `tool_choice` names: any choice that is not one of the keywords the
/// gateway sends. An empty choice is `auto`.
fn named_tool_choice(choice: &str) -> Option<&str> {
    match choice {
        "" | "auto" | "none" | "required" => None,
        name => Some(name),
    }
}

/// Whether the Messages API takes `name` as a tool name unchanged: 1 to 128 ASCII letters,
/// digits, `_` or `-` (its `^[a-zA-Z0-9_-]{1,128}$`).
fn is_wire_tool_name(name: &str) -> bool {
    (1..=TOOL_NAME_MAX_LEN).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// The spelling each tool name of a request goes out under.
///
/// The Messages API refuses a tool name outside `^[a-zA-Z0-9_-]{1,128}$`, and one such
/// name is a 400 for the whole request. The gateway names its tools with dots
/// (`web.search`, `plan.propose`), so those go out under a spelling the API takes, and the
/// calls the model makes under it are named back to the caller. A name the API already
/// takes goes out unchanged, so a request whose names are all accepted is the body it
/// always was.
///
/// Built from the request alone, so the body and the decoding of its response always agree
/// on the same spellings.
#[derive(Debug, Default)]
struct ToolWireNames {
    /// Each name the request carries, as the caller spells it, to its wire spelling.
    to_wire: HashMap<String, String>,
    /// The wire spelling of each renamed name, back to the caller's spelling.
    to_original: HashMap<String, String>,
}

impl ToolWireNames {
    fn for_request(req: &InferRequest) -> Self {
        // Each distinct name once, in request order: the tools, then a forced choice.
        let mut seen: HashSet<&str> = HashSet::new();
        let originals: Vec<&str> = req
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .chain(named_tool_choice(&req.tool_choice))
            .filter(|name| seen.insert(*name))
            .collect();
        // Names the API takes keep their spelling and are claimed before any rewritten name is
        // chosen, so a rewrite never lands on a spelling a caller already uses.
        let mut taken: HashSet<String> = originals
            .iter()
            .copied()
            .filter(|name| is_wire_tool_name(name))
            .map(str::to_owned)
            .collect();
        let mut names = Self::default();
        for original in originals {
            if is_wire_tool_name(original) {
                names.to_wire.insert(original.to_owned(), original.to_owned());
            } else {
                let wire = unique_wire_name(original, &mut taken);
                names.to_wire.insert(original.to_owned(), wire.clone());
                names.to_original.insert(wire, original.to_owned());
            }
        }
        names
    }

    /// The spelling `original` goes out under. A name the request did not carry is sent as
    /// it is given.
    fn wire(&self, original: &str) -> String {
        self.to_wire
            .get(original)
            .cloned()
            .unwrap_or_else(|| original.to_owned())
    }

    /// The caller's spelling of a wire name this request renamed, if it renamed one.
    fn original(&self, wire: &str) -> Option<&str> {
        self.to_original.get(wire).map(String::as_str)
    }

    /// Whether the request carried `name` as it is spelled.
    fn carries(&self, name: &str) -> bool {
        self.to_wire.contains_key(name)
    }
}

/// A spelling of `original` that the API takes and no other name in `taken` uses, which it
/// then claims.
fn unique_wire_name(original: &str, taken: &mut HashSet<String>) -> String {
    // Every character the API refuses becomes `_`, so the spelling still reads as the
    // caller's name. The result is ASCII, so cutting it at a byte offset is safe.
    let base: String = original
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let base = if base.is_empty() {
        "tool".to_owned()
    } else {
        base
    };
    let mut candidate = base[..base.len().min(TOOL_NAME_MAX_LEN)].to_owned();
    let mut suffix = 2_u32;
    while taken.contains(&candidate) {
        let tail = format!("_{suffix}");
        let stem = base.len().min(TOOL_NAME_MAX_LEN - tail.len());
        candidate = format!("{}{tail}", &base[..stem]);
        suffix += 1;
    }
    taken.insert(candidate.clone());
    candidate
}

/// Name each tool call the model made as the caller spelled the tool. A name the request did
/// not carry passes through unchanged, and is logged by name only.
fn restore_tool_names(names: &ToolWireNames, calls: &mut [super::ToolCall]) {
    for call in calls {
        if let Some(original) = names.original(&call.name) {
            call.name = original.to_owned();
        } else if !names.carries(&call.name) {
            debug!(tool = %call.name, "tool call names a tool this request did not offer");
        }
    }
}

/// The current model a retired Claude id is answered by. The model record holds the
/// table and its exact matching, so a date-stamped or unknown id is not guessed at.
fn successor_of_retired_model(model: &str) -> Option<&'static str> {
    model_record::retired_successor(Family::Anthropic, model)
}

/// The request as the current model serves it, when it names a retired id. `None`
/// means "send it as given", which is every request but those few, so the common
/// path copies nothing. The log carries the two ids and never the content.
pub(crate) fn with_current_model(req: &InferRequest) -> Option<InferRequest> {
    let successor = successor_of_retired_model(&req.model)?;
    debug!(
        from = %req.model,
        to = successor,
        "retired Claude model id answered by its successor"
    );
    let mut mapped = req.clone();
    mapped.model = successor.to_owned();
    Some(mapped)
}

/// Write `thinking` (and, for the adaptive class, `output_config`) into the
/// request body, and raise `max_tokens` by the level's allowance: thinking is
/// spent from `max_tokens` on every class, so without the raise a deeper level
/// would only shrink the answer.
fn apply_thinking(body: &mut serde_json::Value, req: &InferRequest, ladder: bool) {
    // The 5.5 models take only the adaptive wire, so they take the ladder whatever
    // the switch says.
    let ladder = ladder || is_claude_5_5(&req.model);
    if !ladder {
        if let Some(budget) =
            resolve_thinking_budget(&req.model, req.thinking_budget_tokens, req.max_tokens)
        {
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget,
            });
        }
        return;
    }
    let Some(resolved) = resolve_ladder_effort(req) else {
        return;
    };
    let max_tokens = effort::raised_ceiling(req.max_tokens, resolved.applied);
    match resolved.wire {
        Wire::AnthropicBudget => {
            let budget = effort::allowance(resolved.applied);
            // Same guard as the legacy path: a budget that leaves no room for
            // an answer is a provider 400.
            if max_tokens <= budget {
                warn!(
                    budget,
                    max_tokens,
                    "extended-thinking budget leaves no room for an answer; omitting"
                );
                return;
            }
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget,
            });
        }
        Wire::AnthropicAdaptive => {
            // `summarized` asks for visible reasoning; the newest models default
            // to omitting it, which would leave the reasoning stream empty.
            body["thinking"] = serde_json::json!({
                "type": "adaptive",
                "display": "summarized",
            });
            body["output_config"] = serde_json::json!({
                "effort": resolved.applied.as_str(),
            });
        }
        // No other wire class is declared for the Anthropic family.
        _ => return,
    }
    // Logged only once a level has reached the body. A budget the no-room guard
    // drops, or a wire this adapter does not render, sends none, so it logs none.
    // Ids and closed tokens only, never the prompt.
    info!(
        request_id = %req.request_id,
        model = %req.model,
        wire = ?resolved.wire,
        requested = resolved.requested.as_str(),
        applied = resolved.applied.as_str(),
        clamped = resolved.clamped(),
        max_tokens,
        "effort_resolved"
    );
    body["max_tokens"] = serde_json::json!(max_tokens);
}

/// Beta header for Anthropic's server-side context-editing feature FAMILY —
/// the `clear_tool_uses_20250919` strategy lives inside this family and is
/// selected in the request body, not by a header of its own. Verified live
/// against platform.claude.com's context-editing reference (Sept 2026).
const CONTEXT_MANAGEMENT_BETA: &str = "context-management-2025-06-27";

/// Beta header for Anthropic's server-side compaction feature
/// (`compact_20260112`). Verified live against platform.claude.com's
/// compaction reference (Sept 2026).
const COMPACTION_BETA: &str = "compact-2026-01-12";

/// Anthropic's floor for the `compact_20260112` trigger's `value` — a smaller
/// number is a `400`, not a more eager trigger. Asserted below, not enforced
/// at runtime: the value this provider sends is a compile-time constant, so a
/// violation is a bug in this file, not a caller input to validate.
const MIN_COMPACTION_TRIGGER_TOKENS: u64 = 50_000;

/// Whether to ask Anthropic's own context-editing (`clear_tool_uses_20250919`)
/// and compaction (`compact_20260112`) to manage this provider's conversation
/// context.
///
/// # Why this exists — the native-compaction migration
///
/// On by default. This REPLACES model-gateway's hand-rolled
/// `clear_stale_tool_results` (tier 1) and `plan_head_summary`/
/// `apply_head_summary` (tier 2) for a turn resolved to THIS provider —
/// model-gateway's own half of this migration skips both for a model this
/// heuristic identifies as Anthropic-family (see
/// `model_gateway::compaction::should_run_local_compaction` and
/// `::clear_stale_tool_results_unless_native`). Leaving both off here would
/// silently reintroduce the unbounded-history growth those two mechanisms
/// exist to prevent, for every Anthropic-routed turn.
///
/// # The escape hatch
///
/// Context editing's Azure/Microsoft Foundry support is confirmed (Microsoft's
/// own Foundry docs list the exact beta header). Compaction's is NOT
/// independently confirmed at the dated-identifier level for an
/// Azure-*hosted* Claude deployment as of the research this migration is
/// built on (Sept 2026) — only inferred, for deployments proxied straight
/// through to Anthropic's own infrastructure. A `4xx` from either beta
/// parameter is handled the same way any other provider rejection already is
/// (the fallback chain logs it, retries, and walks to the next candidate
/// model/provider — see `provider::fallback`) — this flag exists for an
/// operator who has confirmed the rejection is persistent on their resource
/// and wants it stopped at the source rather than repeatedly eating a failed
/// attempt.
fn native_context_management_enabled() -> bool {
    context_management_flag_enabled(
        std::env::var("ANTHROPIC_NATIVE_CONTEXT_MANAGEMENT")
            .ok()
            .as_deref(),
    )
}

/// Pure parser behind [`native_context_management_enabled`], so the on/off
/// decision is unit-testable without mutating process-global environment
/// state — which is racy under Rust's default parallel test runner, and every
/// other env-gated toggle in this codebase that gets tested at all follows
/// the same split (a thin env-reading wrapper over a pure parser).
fn context_management_flag_enabled(raw: Option<&str>) -> bool {
    !matches!(raw.map(str::trim), Some("0" | "false" | "off" | "no"))
}

/// Whether `model` supports Anthropic's `compact_20260112` strategy.
///
/// # Verified live, not assumed
///
/// The research this migration is built on flagged Azure support for
/// compaction specifically (as opposed to context editing) as unconfirmed at
/// the dated-identifier level. Tested live against this deployment's real
/// Azure Foundry resource (Sept 2026): `claude-haiku-4-5` 400s naming the
/// model and strategy explicitly —
/// `"'claude-haiku-4-5-20251001' does not support the 'compact_20260112'
/// context management strategy"` — while `claude-sonnet-4-6` and
/// `claude-opus-4-8` both accept the identical payload and beta header and
/// return `200` with `context_management.applied_edits` and
/// `usage.iterations` present, exactly per the documented response shape.
/// `clear_tool_uses_20250919` showed no such restriction on any tier tested.
///
/// This is a real, not hypothetical, blast radius: `claude-haiku-4-5` is
/// [`DEFAULT_AZURE_ANTHROPIC_MODEL`] — every unspecified-model request on
/// this provider resolves to it — so sending `compact_20260112`
/// unconditionally would 400 the DEFAULT route on every turn, deterministically,
/// once real traffic exceeded the trigger, well past what the existing
/// retry/fallback machinery should have to absorb for a turn that was never
/// going to succeed on this provider.
///
/// Only explicitly verified deployment families are enabled. An unknown alias
/// keeps local gateway recovery rather than inheriting support by its name.
fn model_supports_compaction(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    // Only deployments with verified support; unknown future aliases fail closed.
    ["claude-sonnet-4-6", "claude-opus-4-8"].iter().any(|known| model == *known || model.starts_with(&format!("{known}-")))
}

/// Build the `context_management.edits[]` block that hands this turn's
/// tool-result clearing — and, where the model supports it (see
/// [`model_supports_compaction`]), head compaction too — to Anthropic itself.
///
/// `keep: 3` and the 30k/5k-token thresholds mirror
/// `compaction::DEFAULT_TOOL_PAYLOAD_BUDGET`'s shape (keep the newest 3 tool
/// rounds; only clear once it is worth clearing) so the native behaviour is
/// the same policy in spirit, not a new one model-gateway never asked for.
/// `compact`'s `trigger` uses Anthropic's own documented default (150k input
/// tokens) rather than anything derived from model-gateway's
/// `MAX_THREAD_CONTEXT_MESSAGES`: that constant counts MESSAGES, not tokens,
/// and the two are not commensurable.
///
/// `instructions` for `compact` echoes `compaction::summary_prompt`'s own
/// preservation guidance (constraints, decisions, identifiers) so a native
/// summary keeps the same things model-gateway's hand-rolled one was built to
/// keep — even though this path has no access to the memory `on_pre_compress`
/// hook (`compaction::summary_prompt_with_memory`'s directive), which is
/// fetched from session-core by org/thread id, and this provider-agnostic
/// codec layer never sees either.
fn apply_context_management(body: &mut serde_json::Value, model: &str) {
    // Compile-time, not `debug_assert!`: both operands are constants, so a
    // violation is a bug in the literal below, not something that could ever
    // pass in one build and fail in another.
    const _: () = assert!(
        150_000 >= MIN_COMPACTION_TRIGGER_TOKENS,
        "compact trigger below Anthropic's documented floor"
    );
    let mut edits = vec![serde_json::json!({
        "type": "clear_tool_uses_20250919",
        "trigger": { "type": "input_tokens", "value": 30_000 },
        "keep": { "type": "tool_uses", "value": 3 },
        "clear_at_least": { "type": "input_tokens", "value": 5_000 }
    })];
    if model_supports_compaction(model) {
        edits.push(serde_json::json!({
            "type": "compact_20260112",
            "trigger": { "type": "input_tokens", "value": 150_000 },
            "instructions": "Summarize the earlier part of this conversation so a later turn \
                can continue without it. Preserve, verbatim where possible: constraints and \
                preferences the user stated, decisions already made, identifiers (names, \
                order/invoice/customer numbers, dates, amounts), and anything still \
                unresolved. Omit pleasantries and restating of tool mechanics."
        }));
    }
    body["context_management"] = serde_json::json!({ "edits": edits });
}

/// The `anthropic-beta` header value for a request carrying
/// [`apply_context_management`]'s block for `model`: both features' betas,
/// comma-joined (Anthropic's documented convention for combining beta headers
/// in one request) — or just the context-editing beta alone for a model
/// [`model_supports_compaction`] excludes, so the header never advertises a
/// strategy the body does not actually carry.
fn context_management_beta_header(model: &str) -> String {
    if model_supports_compaction(model) {
        format!("{CONTEXT_MANAGEMENT_BETA},{COMPACTION_BETA}")
    } else {
        CONTEXT_MANAGEMENT_BETA.to_owned()
    }
}

/// Anthropic's own record of what its native context management actually did
/// this turn: `(strategy type, cleared_tool_uses, cleared_input_tokens)` per
/// entry, parsed from `context_management.applied_edits[]`. Present on the
/// unary response object, and — per the compaction reference — on the
/// streaming `message_delta` event; empty whenever neither strategy
/// triggered, which is the common case for an ordinary turn that never grew
/// large enough to need either.
fn applied_context_edits(json: &serde_json::Value) -> Vec<(String, i64, i64)> {
    json["context_management"]["applied_edits"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|edit| {
            (
                edit["type"].as_str().unwrap_or("unknown").to_owned(),
                edit["cleared_tool_uses"].as_i64().unwrap_or(0),
                edit["cleared_input_tokens"].as_i64().unwrap_or(0),
            )
        })
        .collect()
}

/// Surface [`applied_context_edits`] at INFO — the downstream signal the
/// native-compaction migration exists to make observable: whether Anthropic's
/// own context management actually acted this turn, and by how much. Same
/// observability level as the cache-leg log line this sits beside; a `.rs`
/// grep for either finds both halves of the same story.
fn log_applied_context_edits(model: &str, provider: &str, json: &serde_json::Value) {
    for (edit_type, cleared_tool_uses, cleared_input_tokens) in applied_context_edits(json) {
        info!(
            model,
            provider,
            edit_type = %edit_type,
            cleared_tool_uses,
            cleared_input_tokens,
            "anthropic native context management acted on this turn"
        );
    }
}

/// The content blocks of the message at `index` when images ride with it: one
/// `image` block per image, then the text. `None` for a message with no image,
/// which keeps its plain-string content. An empty text is left out, since the
/// API rejects an empty text block.
fn content_with_images(req: &InferRequest, index: usize, text: &str) -> Option<serde_json::Value> {
    let mut blocks: Vec<serde_json::Value> = super::images_for_message(req, index)
        .map(|image| {
            serde_json::json!({
                "type": "image",
                "source": { "type": "base64", "media_type": image.mime_type, "data": image.base64() },
            })
        })
        .collect();
    if blocks.is_empty() {
        return None;
    }
    if !text.is_empty() {
        blocks.push(serde_json::json!({ "type": "text", "text": text }));
    }
    Some(serde_json::Value::Array(blocks))
}

fn build_request_body(req: &InferRequest) -> serde_json::Value {
    build_request_body_with_flags(
        req,
        native_context_management_enabled(),
        tiered_system_enabled(),
    )
}

fn build_request_body_with_context_management(req: &InferRequest, native_enabled: bool) -> serde_json::Value {
    build_request_body_with_flags(req, native_enabled, tiered_system_enabled())
}

fn build_request_body_with_flags(
    req: &InferRequest,
    native_enabled: bool,
    tiered_enabled: bool,
) -> serde_json::Value {
    // A retired id is sent as its successor, so every rule below sees the model
    // that answers the call.
    let current = with_current_model(req);
    let req = current.as_ref().unwrap_or(req);
    // The Anthropic Messages API takes the system prompt as a TOP-LEVEL `system`
    // parameter, not as a message with role "system" — sending it inline 400s:
    // "messages.0: use the top-level 'system' parameter for the initial system
    // prompt". Split the OpenAI-style flat message list: system turns are joined
    // into `system`; only user/assistant turns are forwarded as `messages`.
    let system: String = req
        .messages
        .iter()
        .filter(|m| m.role.eq_ignore_ascii_case("system"))
        .map(|m| m.content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    let messages: Vec<serde_json::Value> = req
        .messages
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.role.eq_ignore_ascii_case("system"))
        .map(|(index, m)| {
            // The index is taken before the system messages are filtered out,
            // so it names the same message the caller's `image_parts` did.
            if let Some(content) = content_with_images(req, index, &m.content) {
                return serde_json::json!({"role": m.role, "content": content});
            }
            if m.role == "assistant" && !m.compaction_summary.trim().is_empty()
                && model_supports_compaction(&req.model) && native_enabled {
                return serde_json::json!({"role": "assistant", "content": [
                    {"type": "compaction", "content": m.compaction_summary},
                    {"type": "text", "text": m.content}
                ]});
            }
            serde_json::json!({
                "role": m.role,
                "content": m.content,
            })
        })
        .collect();

    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens,
    });
    apply_thinking(&mut body, req, adaptive_thinking_enabled());
    // No sampling parameter is sent to any model. On API version 2026-10-21 all three
    // are refused everywhere; on 2023-06-01 the 5.5 models refuse or narrow them
    // (Haiku 5.5 takes only temperature 1 and top_p 0.99), so leaving them out is the
    // one choice that is safe on either version.
    // `temperature` is deliberately omitted. Anthropic constrains it to [0, 1]
    // (vs OpenAI's [0, 2]) and the newest Claude models reject it outright
    // ("`temperature` is deprecated for this model" — e.g. claude-opus-4-8), so
    // forwarding a chat temperature 400s on those. Anthropic's own default
    // sampling is used instead; structured output is steered via the prompt
    // (see the response_format note above), not the temperature.
    // A ZDR request never takes the tiered form: it has nothing to mark, and
    // keeping the plain string means no structured rewrite can hint at caching.
    let tiered = if tiered_enabled && !req.zdr {
        tiered_system_blocks(req)
    } else {
        None
    };
    if let Some(blocks) = tiered {
        body["system"] = serde_json::Value::Array(
            blocks
                .into_iter()
                .map(|(_, text)| serde_json::json!({ "type": "text", "text": text }))
                .collect(),
        );
    } else if !system.is_empty() {
        body["system"] = serde_json::Value::String(system);
    }

    // The Anthropic Messages API has no OpenAI-style json-schema `response_format`
    // and rejects unknown `metadata` keys (it 400s with
    // "metadata.structured_output_schema: Extra inputs are not permitted"). A
    // requested `structured_output_schema` is therefore honored via the caller's
    // prompt (e.g. "Reply with ONLY a JSON object" + the schema in-context)
    // rather than forwarded as an API parameter — sending it broke every
    // structured-output inference (e.g. the onboarding plan recommendation).

    // chat-parity §2 function-calling: translate tool definitions to the
    // Anthropic `tools`/`tool_choice` shape. Empty → omitted.
    if !req.tools.is_empty() {
        // The spellings the API accepts, and the forced choice under the same spellings.
        let names = ToolWireNames::for_request(req);
        let tools: Vec<serde_json::Value> = req
            .tools
            .iter()
            .map(|t| {
                let schema = super::tool_parameters(&t.name, &t.parameters_json);
                serde_json::json!({
                    "name": names.wire(&t.name),
                    "description": t.description,
                    "input_schema": schema,
                })
            })
            .collect();
        body["tools"] = serde_json::Value::Array(tools);
        let tc = if req.tool_choice.is_empty() {
            "auto"
        } else {
            req.tool_choice.as_str()
        };
        let mut choice = tool_choice_wire(&req.model, tc);
        if let Some(name) = choice["name"].as_str().map(str::to_owned) {
            choice["name"] = serde_json::Value::String(names.wire(&name));
        }
        body["tool_choice"] = choice;
    }

    // Native token-based compaction complements gateway message-count/tool
    // budgets. Not gated on `req.zdr`: unlike prompt caching, this
    // adds no NEW cross-request retention — it only shapes what THIS single
    // call's context looks like to the model.
    if native_enabled {
        apply_context_management(&mut body, &req.model);
        // Tool decision rounds are not persisted as assistant turns. Until they
        // have durable checkpoints, compact only the final answer request.
        if !req.tools.is_empty() {
            if let Some(edits) = body["context_management"]["edits"].as_array_mut() {
                edits.retain(|edit| edit["type"] != "compact_20260112");
            }
        }
    }

    // Mark the stable prefix for Anthropic's prompt cache. Must run last: it
    // rewrites `tools` / `system` / `messages` in place, so every field it marks
    // has to already be present. Bypassed entirely for ZDR requests.
    apply_prompt_caching(&mut body, req);

    body
}

/// Parse Anthropic `tool_use` content blocks into the internal [`ToolCall`].
fn parse_tool_calls(json: &serde_json::Value) -> Vec<super::ToolCall> {
    json["content"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|b| b["type"].as_str() == Some("tool_use"))
                .filter_map(|b| {
                    let id = b["id"].as_str()?.to_owned();
                    let name = b["name"].as_str()?.to_owned();
                    let arguments_json =
                        serde_json::to_string(&b["input"]).unwrap_or_else(|_| "{}".to_owned());
                    Some(super::ToolCall {
                        id,
                        name,
                        arguments_json,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the Anthropic response JSON into our unified response type.
fn to_i32_or_max(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// Tokens served from Anthropic's prompt cache for this request (0 when
/// absent). `usage` is the Anthropic `usage` object itself (found at
/// `response.usage` for non-streaming replies, `message_start.message.usage`
/// for streaming).
fn cache_read_input_tokens(usage: &serde_json::Value) -> i32 {
    usage_tokens(usage, "cache_read_input_tokens")
}

/// Tokens newly written to Anthropic's prompt cache by this request (0 when
/// absent). See [`cache_read_input_tokens`] for the `usage` shape.
fn cache_creation_input_tokens(usage: &serde_json::Value) -> i32 {
    usage_tokens(usage, "cache_creation_input_tokens")
}

// Compaction usage is excluded from top-level usage. Sum iterations once;
// otherwise native compaction would hide billed work from cost accounting.
fn usage_tokens(usage: &serde_json::Value, field: &str) -> i32 {
    if let Some(iterations) = usage["iterations"].as_array().filter(|items| !items.is_empty()) {
        iterations.iter().fold(0i32, |total, item| total.saturating_add(to_i32_or_max(item[field].as_i64().unwrap_or(0))))
    } else { to_i32_or_max(usage[field].as_i64().unwrap_or(0)) }
}

/// With prompt caching on, Anthropic's `input_tokens` counts ONLY the tokens
/// *after* the last cache breakpoint — the cached prefix is reported
/// separately as `cache_read_input_tokens` (served from cache) and
/// `cache_creation_input_tokens` (newly written). Downstream cost accounting
/// reads this as the request's total input, so the cache legs are folded back
/// in: without this, enabling caching would silently drop most of every
/// round's input from usage and under-report cost. Both cache fields are
/// absent (→ 0) when nothing was cached, so uncached requests are unchanged.
fn total_input_tokens(usage: &serde_json::Value) -> i32 {
    usage_tokens(usage, "input_tokens")
        .saturating_add(cache_read_input_tokens(usage))
        .saturating_add(cache_creation_input_tokens(usage))
}

/// This `message_delta` SSE event's `stop_reason`, if set.
///
/// Anthropic's streaming `stop_reason` is `null` in `message_start` and
/// arrives ONLY on `message_delta` (nested under `delta`, not top-level like
/// the unary response's `json["stop_reason"]`) — never on any other event.
/// Verified against platform.claude.com's SSE/streaming reference, not
/// assumed.
fn anthropic_message_delta_stop_reason(json: &serde_json::Value) -> Option<&str> {
    json["delta"]["stop_reason"].as_str()
}

fn parse_response(request_id: &str, json: &serde_json::Value) -> InferResponse {
    // Concatenate all text blocks (a response may interleave text + tool_use).
    let content = json["content"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    let model_used = json["model"].as_str().unwrap_or("unknown").to_owned();
    let stop_reason = json["stop_reason"]
        .as_str()
        .unwrap_or("end_turn")
        .to_owned();
    let input_tokens = total_input_tokens(&json["usage"]);
    let output_tokens = usage_tokens(&json["usage"], "output_tokens");
    let tool_calls = parse_tool_calls(json);

    InferResponse {
        compaction_summary: json["content"].as_array().into_iter().flatten()
            .filter(|block| block["type"] == "compaction")
            .filter_map(|block| block["content"].as_str())
            .last().unwrap_or_default().to_owned(),
        request_id: request_id.to_owned(),
        content,
        model_used,
        stop_reason,
        input_tokens,
        output_tokens,
        tool_calls,
        // Provenance is stamped by the fallback chain (which knows which
        // registered provider served), not by the raw adapter.
        provider_used: String::new(),
        residency: String::new(),
        // Anthropic does not report token logprobs on any endpoint, so
        // there is no model-certainty signal to carry here; `None` reads as
        // "unknown" downstream, never as low confidence.
        token_confidence: None,
        cache_read_input_tokens: cache_read_input_tokens(&json["usage"]),
        cache_creation_input_tokens: cache_creation_input_tokens(&json["usage"]),
        provider_timing: None,
        service_tier_used: String::new(),
    }
}

/// The typed error for a non-success Messages response.
///
/// Overflow keeps its own type, which sends the chain on to a provider with a larger window.
/// A refusal of the request itself is typed `RequestRejected`, so the chain does not send the
/// same request to this provider again.
fn classify_failure(status: reqwest::StatusCode, body: &str) -> ProviderError {
    match crate::provider::overflow::classify_http_failure(status, body) {
        ProviderError::Http(detail) if refuses_request(status) => {
            ProviderError::RequestRejected { detail }
        }
        typed => typed,
    }
}

/// Whether `status` is the API refusing the request: a client error that a second send would
/// get again. A 429 is read as a rate limit before this is reached. Timeouts (408), conflicts
/// (409) and auth failures (401, 403) can clear without any change to the request, and every
/// 5xx is a server fault, so all of those keep the generic error and its retries.
fn refuses_request(status: reqwest::StatusCode) -> bool {
    status.is_client_error() && !matches!(status.as_u16(), 401 | 403 | 408 | 409 | 429)
}

#[allow(clippy::too_many_lines)]
#[async_trait::async_trait]
impl ProviderRouter for AnthropicProvider {
    fn capabilities(&self) -> super::ProviderCapabilities {
        // Claude: tools, vision, extended thinking, streaming; 1M context on the direct
        // 5.5 catalogue and 200k on Foundry.
        // No first-party embeddings API.
        super::ProviderCapabilities {
            provider_id: self.provider_name().to_owned(),
            // `claude` addresses either flavor; `anthropic` is the historical
            // spelling callers send for the Foundry deployment.
            aliases: match &self.flavor {
                AnthropicFlavor::Direct => vec!["claude".to_owned()],
                AnthropicFlavor::Azure { .. } => {
                    vec!["claude".to_owned(), "anthropic".to_owned()]
                }
            },
            model_family: super::ModelFamily::Anthropic,
            residency: self.residency,
            exclusive_catalog: false,
            supports_tools: true,
            supports_vision: true,
            supports_thinking: true,
            supports_streaming: true,
            supports_embeddings: false,
            supports_zdr: self.zdr.as_deref().is_some_and(ZdrAttestation::in_force_now),
            modalities: vec!["chat".to_owned(), "vision".to_owned()],
            max_context_tokens: match &self.flavor {
                AnthropicFlavor::Direct => 1_000_000,
                AnthropicFlavor::Azure { .. } => 200_000,
            },
            // 128K is the streamed ceiling. A unary call above about 16K output can outlast
            // the provider read timeout (120 s by default), so long outputs should stream.
            max_output_tokens: match &self.flavor {
                AnthropicFlavor::Direct => 128_000,
                AnthropicFlavor::Azure { .. } => 8_192,
            },
        }
    }

    /// Advertise the Claude chat catalog so `/v1/models` is populated (the SPA
    /// model picker reads this). Direct uses the first-party model ids; Azure
    /// Foundry uses the deployed Claude deployment names from config.
    fn list_models(&self) -> Vec<ModelInfo> {
        let ladder = adaptive_thinking_enabled();
        let provider = self.provider_name().to_owned();
        let ids: Vec<String> = match &self.flavor {
            AnthropicFlavor::Direct => [
                DEFAULT_ANTHROPIC_MODEL,
                CLAUDE_OPUS_5_5,
                CLAUDE_HAIKU_5_5,
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            AnthropicFlavor::Azure { models, .. } => models.clone(),
        };
        // Venice-style per-model privacy disclosure, derived from this
        // provider's own declared residency + ZDR attestation.
        let caps = self.capabilities();
        let aliases = effort::ModelAliases::from_env();
        let provider_features = vec![
            "tools".to_owned(),
            "vision".to_owned(),
            "reasoning".to_owned(),
        ];
        let privacy_tier = super::PrivacyTier::classify(&caps);
        let residency_label = if caps.residency == super::Residency::Global {
            String::new()
        } else {
            caps.residency.as_str().to_owned()
        };
        ids.into_iter()
            .map(|id| {
                let cheap = is_cheap_claude(&id);
                let effort_levels = advertised_effort_levels(&id, ladder);
                let features = super::features_for_model(&caps, &provider_features, &id);
                let record = model_record::advertised(
                    Family::Anthropic,
                    &aliases,
                    &id,
                    caps.max_context_tokens,
                    caps.max_output_tokens,
                );
                ModelInfo {
                    id,
                    provider: provider.clone(),
                    modality: "chat".to_owned(),
                    streaming: true,
                    features,
                    cheap,
                    privacy_tier,
                    residency_label: residency_label.clone(),
                    effort_levels,
                    context_window_tokens: record.context_window_tokens,
                    max_output_tokens: record.max_output_tokens,
                    lifecycle: record.lifecycle,
                    successor: record.successor,
                    priority_tier: record.priority_tier,
                }
            })
            .collect()
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        // The header and the logs below name the model that answers, as the body does.
        let current = with_current_model(req);
        let req = current.as_ref().unwrap_or(req);
        let body = build_request_body(req);

        let mut request = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json");
        if native_context_management_enabled() {
            request = request.header("anthropic-beta", context_management_beta_header(&req.model));
        }
        let response = request
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        // Check rate-limit headers
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                // Default 1s (matches the OpenAI provider), NOT 1000s — the old
                // unwrap_or(1000) then *1000 stalled retries for ~17 minutes.
                .unwrap_or(1);
            return Err(ProviderError::RateLimited {
                retry_after_ms: retry_after * 1000,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "no body".to_owned());
            return Err(classify_failure(status, &text));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(e.to_string()))?;

        // The cache legs are logged so a real multi-round turn can be verified:
        // round 1 shows a non-zero `cache_creation_input_tokens`, round 2+ a
        // non-zero `cache_read_input_tokens`. Both stay 0 for ZDR requests,
        // which never carry a breakpoint.
        info!(
            model = %req.model,
            provider = self.provider_name(),
            cache_read_input_tokens = cache_read_input_tokens(&json["usage"]),
            cache_creation_input_tokens = cache_creation_input_tokens(&json["usage"]),
            "infer completed"
        );
        log_applied_context_edits(&req.model, self.provider_name(), &json);
        let mut response = parse_response(&req.request_id, &json);
        restore_tool_names(&ToolWireNames::for_request(req), &mut response.tool_calls);
        Ok(response)
    }

    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        let current = with_current_model(req);
        let req = current.as_ref().unwrap_or(req);
        let mut body = build_request_body(req);
        body["stream"] = serde_json::Value::Bool(true);

        let mut request = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json");
        if native_context_management_enabled() {
            request = request.header("anthropic-beta", context_management_beta_header(&req.model));
        }
        let response = request
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                // Default 1s (matches the OpenAI provider), NOT 1000s — the old
                // unwrap_or(1000) then *1000 stalled retries for ~17 minutes.
                .unwrap_or(1);
            return Err(ProviderError::RateLimited {
                retry_after_ms: retry_after * 1000,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "no body".to_owned());
            return Err(classify_failure(status, &text));
        }

        let request_id = req.request_id.clone();
        let model = req.model.clone();
        let provider_name = self.provider_name().to_owned();
        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(async move {
            // Read SSE events from the response body.
            // Anthropic sends `event: content_block_delta` with `data: {...}` lines.
            let mut bytes_stream = response.bytes_stream();
            let mut buffer = String::new();

            // Anthropic's streaming `message_stop` event carries NO `usage`
            // field — usage is split across two earlier events instead:
            // `message_start.message.usage` has `input_tokens` (plus the cache
            // legs), and `message_delta.usage` has the final cumulative
            // `output_tokens`. Capture both here and use them once the stream
            // ends; reading `usage` off `message_stop` itself (the previous
            // bug) always resolved to 0.
            let mut input_tokens: i32 = 0;
            let mut output_tokens: i32 = 0;
            // Prompt-cache legs, captured off `message_start.message.usage`
            // alongside `input_tokens` (same event, same fold-in relationship
            // documented on `total_input_tokens`) and carried to every final
            // chunk below. This is the streaming path's half of cache-token
            // telemetry: `parse_response` (the unary path) has logged these
            // for a while, but until now nothing on `infer_stream` reported
            // them at all -- a cache hit was unobservable on this path.
            let mut cache_read_tokens: i32 = 0;
            let mut cache_creation_tokens: i32 = 0;
            // Set from `message_delta.delta.stop_reason` once it arrives.
            // Stays empty if the loop exits without ever seeing one -- the
            // tail-chunk fallback below treats that as an incomplete stream,
            // not a natural completion.
            let mut stop_reason = String::new();
            let mut compaction_summary = String::new();

            while let Some(chunk_result) = bytes_stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        warn!(error = %e, "stream read error");
                        break;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&bytes));

                // Process complete SSE lines
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_owned();
                    buffer = buffer[newline_pos + 1..].to_owned();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            let final_chunk = InferChunk {
                                compaction_summary: compaction_summary.clone(),
                                reasoning_delta: String::new(),
                                request_id: request_id.clone(),
                                delta: String::new(),
                                done: true,
                                model_used: model.clone(),
                                input_tokens,
                                output_tokens,
                                provider_used: String::new(),
                                residency: String::new(),
                                token_confidence: None,
                                stop_reason: if stop_reason.is_empty() {
                                    "end_turn".to_owned()
                                } else {
                                    stop_reason.clone()
                                },
                                cache_read_input_tokens: cache_read_tokens,
                                cache_creation_input_tokens: cache_creation_tokens,
                                provider_timing: None,
                                service_tier_used: String::new(),
                            };
                            let _ = tx.send(final_chunk).await;
                            return;
                        }

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            let event_type = json["type"].as_str().unwrap_or("");

                            if event_type == "message_start" {
                                input_tokens = total_input_tokens(&json["message"]["usage"]);
                                cache_read_tokens =
                                    cache_read_input_tokens(&json["message"]["usage"]);
                                cache_creation_tokens =
                                    cache_creation_input_tokens(&json["message"]["usage"]);
                            } else if event_type == "message_delta" {
                                if json["usage"]["output_tokens"].is_number() || json["usage"]["iterations"].is_array() {
                                    output_tokens = usage_tokens(&json["usage"], "output_tokens");
                                }
                                if json["usage"]["input_tokens"].is_number() || json["usage"]["iterations"].is_array() {
                                    input_tokens = total_input_tokens(&json["usage"]);
                                    cache_read_tokens = cache_read_input_tokens(&json["usage"]);
                                    cache_creation_tokens = cache_creation_input_tokens(&json["usage"]);
                                }
                                if let Some(reason) = anthropic_message_delta_stop_reason(&json) {
                                    stop_reason = reason.to_owned();
                                }
                                // Per the compaction reference, `applied_edits`
                                // arrives on this event when streaming (the
                                // unary path reads it off the whole response
                                // instead -- see `infer`'s own call to this).
                                log_applied_context_edits(&model, &provider_name, &json);
                            } else if event_type == "content_block_start" && json["content_block"]["type"] == "compaction" {
                                compaction_summary = json["content_block"]["content"].as_str().unwrap_or_default().to_owned();
                            } else if event_type == "content_block_delta" {
                                if json["delta"]["type"] == "compaction_delta" {
                                    compaction_summary.push_str(json["delta"]["content"].as_str().unwrap_or_default());
                                    continue;
                                }
                                // A delta belongs to exactly ONE channel. With
                                // extended thinking enabled the stream
                                // interleaves `thinking_delta` blocks with
                                // `text_delta` ones, and reading `delta.text`
                                // unconditionally — as this did — turned every
                                // thinking delta into an EMPTY answer chunk:
                                // the model's reasoning silently discarded, and
                                // a run of no-op chunks sent in its place.
                                let (delta, reasoning_delta) =
                                    split_content_block_delta(&json["delta"]);
                                if delta.is_empty() && reasoning_delta.is_empty() {
                                    continue;
                                }
                                let chunk = InferChunk {
                                    compaction_summary: String::new(),
                                    request_id: request_id.clone(),
                                    delta,
                                    done: false,
                                    model_used: model.clone(),
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    token_confidence: None,
                                    stop_reason: String::new(),
                                    reasoning_delta,
                                    cache_read_input_tokens: 0,
                                    cache_creation_input_tokens: 0,
                                    provider_timing: None,
                                    service_tier_used: String::new(),
                                };
                                if tx.send(chunk).await.is_err() {
                                    return;
                                }
                            } else if event_type == "message_stop" {
                                let final_chunk = InferChunk {
                                    compaction_summary: compaction_summary.clone(),
                                    reasoning_delta: String::new(),
                                    request_id: request_id.clone(),
                                    delta: String::new(),
                                    done: true,
                                    model_used: model.clone(),
                                    input_tokens,
                                    output_tokens,
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    token_confidence: None,
                                    stop_reason: if stop_reason.is_empty() {
                                        "end_turn".to_owned()
                                    } else {
                                        stop_reason.clone()
                                    },
                                    cache_read_input_tokens: cache_read_tokens,
                                    cache_creation_input_tokens: cache_creation_tokens,
                                    provider_timing: None,
                                    service_tier_used: String::new(),
                                };
                                let _ = tx.send(final_chunk).await;
                                return;
                            }
                        }
                    }
                }
            }

            // End of stream without explicit done marker -- the abnormal
            // path (a transport error above `break`s into it, and so does a
            // connection that just closes early). A stop_reason seen on a
            // real message_delta event still wins, but absent that,
            // "end_turn" would misreport an incomplete answer as a clean one.
            let final_chunk = InferChunk {
                compaction_summary: String::new(),
                reasoning_delta: String::new(),
                request_id,
                delta: String::new(),
                done: true,
                model_used: model,
                input_tokens,
                output_tokens,
                provider_used: String::new(),
                residency: String::new(),
                token_confidence: None,
                stop_reason: if stop_reason.is_empty() {
                    "stream_incomplete".to_owned()
                } else {
                    stop_reason
                },
                cache_read_input_tokens: cache_read_tokens,
                cache_creation_input_tokens: cache_creation_tokens,
                provider_timing: None,
                service_tier_used: String::new(),
            };
            let _ = tx.send(final_chunk).await;
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tool_tests {
    use super::{build_request_body, parse_tool_calls};
    use crate::provider::{InferRequest, ToolDefinition};

    #[test]
    fn build_request_body_includes_tools_in_anthropic_shape() {
        let req = InferRequest {
            thinking_budget_tokens: 0,
            model: "claude-sonnet-4-6".to_owned(),
            max_tokens: 1024,
            tools: vec![ToolDefinition {
                name: "get_weather".to_owned(),
                description: "Get weather".to_owned(),
                parameters_json: r#"{"type":"object","properties":{"city":{"type":"string"}}}"#
                    .to_owned(),
            }],
            tool_choice: "required".to_owned(),
            ..Default::default()
        };
        let body = build_request_body(&req);
        assert_eq!(body["tools"][0]["name"], "get_weather");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        // "required" maps to Anthropic's "any".
        assert_eq!(body["tool_choice"]["type"], "any");
    }

    #[test]
    fn parse_tool_calls_extracts_tool_use_blocks() {
        let json = serde_json::json!({
            "content": [
                { "type": "text", "text": "Let me check." },
                { "type": "tool_use", "id": "tu_1", "name": "get_weather", "input": { "city": "Oslo" } }
            ]
        });
        let calls = parse_tool_calls(&json);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].name, "get_weather");
        assert!(calls[0].arguments_json.contains("Oslo"));
    }

    #[test]
    fn build_request_body_hoists_system_and_omits_temperature() {
        use crate::provider::ChatMessage;
        let req = InferRequest {
            thinking_budget_tokens: 0,
            model: "claude-opus-4-8".to_owned(),
            max_tokens: 256,
            // Newest Claude models reject `temperature` — it must not be sent.
            temperature: 0.7,
            messages: vec![
                ChatMessage {
                    compaction_summary: String::new(),
                    role: "system".to_owned(),
                    content: "You are Verevon.".to_owned(),
                    name: String::new(),
                },
                ChatMessage {
                    compaction_summary: String::new(),
                    role: "user".to_owned(),
                    content: "Hi".to_owned(),
                    name: String::new(),
                },
            ],
            ..Default::default()
        };
        let body = build_request_body(&req);
        // System prompt hoisted to the top-level parameter, removed from messages.
        assert_eq!(body["system"], "You are Verevon.");
        assert_eq!(body["messages"].as_array().expect("messages").len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
        // temperature is never forwarded to Anthropic.
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn anthropic_body_indexes_before_system_filter() {
        use crate::provider::{ChatMessage, ImagePart};
        let message = |role: &str, content: &str| ChatMessage {
            compaction_summary: String::new(),
            role: role.to_owned(),
            content: content.to_owned(),
            name: String::new(),
        };
        let req = InferRequest {
            model: "claude-opus-4-8".to_owned(),
            max_tokens: 256,
            messages: vec![
                message("system", "You are Verevon."),
                message("user", "first"),
                message("assistant", "ok"),
                message("user", "what is this"),
            ],
            // Index 3 of the ORIGINAL list; after the system message is hoisted it
            // is the third of the messages Anthropic receives.
            image_parts: vec![ImagePart {
                message_index: 3,
                data: std::sync::Arc::from(&b"one"[..]),
                mime_type: "image/webp".to_owned(),
            }],
            ..Default::default()
        };
        let body = super::build_request_body_with_flags(&req, false, false);
        let messages = body["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["content"], "first", "other messages keep a string");
        assert_eq!(messages[1]["content"], "ok");
        let blocks = messages[2]["content"].as_array().expect("blocks");
        assert_eq!(
            blocks[0],
            serde_json::json!({
                "type": "image",
                "source": {"type": "base64", "media_type": "image/webp", "data": "b25l"},
            })
        );
        assert_eq!(blocks[1], serde_json::json!({"type": "text", "text": "what is this"}));
    }
}

#[cfg(test)]
mod tool_name_wire_tests {
    //! The tool names on the Messages wire. The gateway's dotted names are a 400 there (see
    //! `ToolWireNames`). The calls run against a local mock of the API, so the body sent and
    //! the names read back are the ones a real call would carry.
    use super::{
        build_request_body_with_flags, classify_failure, is_wire_tool_name, AnthropicProvider,
        ProviderRouter, ToolWireNames,
    };
    use crate::provider::{ChatMessage, InferRequest, ProviderError, ToolDefinition};
    use std::collections::HashSet;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const SCHEMA: &str = r#"{"type":"object","properties":{"city":{"type":"string"}}}"#;

    /// The tools the gateway sends in one request: dotted names the API refuses, beside valid
    /// names that a rewritten dotted name would collide with.
    const GATEWAY_TOOLS: [&str; 6] = [
        "plan.propose",
        "web.search",
        "web_search",
        "subagent.research_fan_out",
        "web.read",
        "web_read",
    ];

    /// What `GATEWAY_TOOLS` goes out as. A rewrite that would collide with a valid name takes
    /// the next free suffix, and the valid name keeps its spelling.
    const GATEWAY_WIRE_NAMES: [&str; 6] = [
        "plan_propose",
        "web_search_2",
        "web_search",
        "subagent_research_fan_out",
        "web_read_2",
        "web_read",
    ];

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_owned(),
            description: format!("the {name} tool"),
            parameters_json: SCHEMA.to_owned(),
        }
    }

    fn gateway_request() -> InferRequest {
        InferRequest {
            model: "claude-sonnet-4-6".to_owned(),
            max_tokens: 1024,
            tools: GATEWAY_TOOLS.into_iter().map(tool).collect(),
            tool_choice: "web.read".to_owned(),
            ..InferRequest::default()
        }
    }

    fn names_in(body: &serde_json::Value) -> Vec<String> {
        body["tools"]
            .as_array()
            .expect("a tools array")
            .iter()
            .map(|entry| entry["name"].as_str().expect("a tool name").to_owned())
            .collect()
    }

    /// Every name the API would refuse goes out under one it accepts, and no two tools share
    /// a wire name. A forced choice goes out under the wire name of the tool it names, and each
    /// wire name decodes back to exactly what the caller spelled.
    #[test]
    fn dotted_tool_names_go_out_as_names_the_api_accepts_and_decode_back() {
        let req = gateway_request();
        let body = build_request_body_with_flags(&req, false, false);
        let sent = names_in(&body);

        assert_eq!(sent, GATEWAY_WIRE_NAMES);
        for name in &sent {
            assert!(is_wire_tool_name(name), "the API refuses {name}");
        }
        let distinct: HashSet<&String> = sent.iter().collect();
        assert_eq!(distinct.len(), sent.len(), "two tools share a wire name");
        assert_eq!(
            body["tool_choice"],
            serde_json::json!({"type": "tool", "name": "web_read_2"})
        );

        let names = ToolWireNames::for_request(&req);
        let decoded: Vec<&str> = sent
            .iter()
            .map(|wire| names.original(wire).unwrap_or(wire.as_str()))
            .collect();
        assert_eq!(decoded, GATEWAY_TOOLS);
    }

    /// A request whose names the API already takes is the body the adapter has always built
    /// for it: the same tools in order, and an automatic choice.
    #[test]
    fn a_request_with_accepted_names_sends_the_same_body_as_before() {
        let req = InferRequest {
            model: "claude-sonnet-4-6".to_owned(),
            max_tokens: 256,
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "Hei".to_owned(),
                name: String::new(),
            }],
            tools: vec![tool("get_weather"), tool("web_search")],
            tool_choice: "auto".to_owned(),
            ..InferRequest::default()
        };
        let body = build_request_body_with_flags(&req, false, false);
        assert_eq!(
            body,
            serde_json::json!({
                "model": "claude-sonnet-4-6",
                "max_tokens": 256,
                "messages": [{"role": "user", "content": "Hei"}],
                "tools": [
                    {
                        "name": "get_weather",
                        "description": "the get_weather tool",
                        "input_schema": crate::provider::tool_parameters("get_weather", SCHEMA),
                    },
                    {
                        "name": "web_search",
                        "description": "the web_search tool",
                        "input_schema": crate::provider::tool_parameters("web_search", SCHEMA),
                    },
                ],
                "tool_choice": {"type": "auto"},
            })
        );
    }

    /// A tool call the model makes under a wire name comes back under the caller's spelling,
    /// and a name the request never carried passes through unchanged.
    #[tokio::test]
    async fn a_unary_tool_call_is_named_back_to_the_caller() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/anthropic/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model": "claude-sonnet-4-6",
                "stop_reason": "tool_use",
                "content": [
                    {"type": "tool_use", "id": "tu_1", "name": "plan_propose", "input": {}},
                    {"type": "tool_use", "id": "tu_2", "name": "web_search", "input": {}},
                    {"type": "tool_use", "id": "tu_3", "name": "made_up", "input": {}},
                ],
                "usage": {"input_tokens": 10, "output_tokens": 2},
            })))
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new_azure("key", server.uri(), vec![])
            .expect("azure-flavored provider for the mock endpoint");
        let req = InferRequest {
            model: "claude-sonnet-4-6".to_owned(),
            max_tokens: 256,
            tools: vec![tool("plan.propose"), tool("web_search")],
            ..InferRequest::default()
        };

        let response = provider.infer(&req).await.expect("the mock answers");
        let called: Vec<&str> = response
            .tool_calls
            .iter()
            .map(|call| call.name.as_str())
            .collect();
        assert_eq!(called, ["plan.propose", "web_search", "made_up"]);

        let received = server.received_requests().await.expect("requests are recorded");
        let sent: serde_json::Value =
            serde_json::from_slice(&received[0].body).expect("the request body is JSON");
        assert_eq!(names_in(&sent), ["plan_propose", "web_search"]);
    }

    /// The streamed request carries the same wire names as the unary one.
    #[tokio::test]
    async fn a_streamed_request_sends_the_same_wire_names() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/anthropic/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                concat!(
                    "event: message_start\n",
                    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",",
                    "\"model\":\"claude-sonnet-4-6\",\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n",
                    "event: message_stop\n",
                    "data: {\"type\":\"message_stop\"}\n\n"
                ),
                "text/event-stream",
            ))
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new_azure("key", server.uri(), vec![])
            .expect("azure-flavored provider for the mock endpoint");

        let mut stream = provider
            .infer_stream(&gateway_request())
            .await
            .expect("the stream starts");
        assert!(stream.recv().await.expect("the final chunk").done);

        let received = server.received_requests().await.expect("requests are recorded");
        let sent: serde_json::Value =
            serde_json::from_slice(&received[0].body).expect("the request body is JSON");
        assert_eq!(names_in(&sent), GATEWAY_WIRE_NAMES);
    }

    /// A 400 that is not an overflow is the API refusing the request. Both routes type it as a
    /// rejection, so the chain does not send the same request to this provider again.
    #[tokio::test]
    async fn a_refused_request_is_a_rejection_on_both_routes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/anthropic/v1/messages"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": "tools.0.custom.name: String should match pattern",
                },
            })))
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new_azure("key", server.uri(), vec![])
            .expect("azure-flavored provider for the mock endpoint");
        let req = gateway_request();

        let unary = provider
            .infer(&req)
            .await
            .expect_err("the API refuses the request");
        assert!(
            matches!(&unary, ProviderError::RequestRejected { detail }
                if detail.contains("tools.0.custom.name")),
            "{unary:?}"
        );
        let streamed = provider
            .infer_stream(&req)
            .await
            .err()
            .expect("the API refuses the stream too");
        assert!(
            matches!(streamed, ProviderError::RequestRejected { .. }),
            "{streamed:?}"
        );
    }

    /// An overflow is still an overflow: the 400 that says so keeps the typed error the chain
    /// walks on to a larger window with.
    #[tokio::test]
    async fn an_overflow_400_keeps_its_typed_overflow() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/anthropic/v1/messages"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": "prompt is too long: 210000 tokens > 200000 maximum",
                },
            })))
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new_azure("key", server.uri(), vec![])
            .expect("azure-flavored provider for the mock endpoint");

        let err = provider
            .infer(&gateway_request())
            .await
            .expect_err("the API refuses the prompt");
        assert!(matches!(err, ProviderError::TooLong { .. }), "{err:?}");
    }

    /// Only a refusal of the request is typed as one. Auth failures, timeouts, conflicts, rate
    /// limits and server errors keep the generic error, which the chain retries.
    #[test]
    fn only_a_refusal_of_the_request_is_typed_as_a_rejection() {
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"no"}}"#;
        for code in [400, 404, 413, 422] {
            let status = reqwest::StatusCode::from_u16(code).expect("a valid status");
            assert!(
                matches!(
                    classify_failure(status, body),
                    ProviderError::RequestRejected { .. }
                ),
                "{code} refuses the request"
            );
        }
        for code in [401, 403, 408, 409, 429, 500, 502, 503, 529] {
            let status = reqwest::StatusCode::from_u16(code).expect("a valid status");
            assert!(
                matches!(classify_failure(status, body), ProviderError::Http(_)),
                "{code} keeps its retries"
            );
        }
        assert!(matches!(
            classify_failure(reqwest::StatusCode::BAD_REQUEST, "prompt is too long"),
            ProviderError::TooLong { .. }
        ));
    }
}

#[cfg(test)]
mod prompt_cache_tests {
    use super::{
        anthropic_message_delta_stop_reason, build_request_body, build_request_body_with_flags,
        min_cacheable_tokens, parse_response, tiered_system_flag_enabled, total_input_tokens,
        MAX_CACHE_BREAKPOINTS,
    };
    use crate::provider::{ChatMessage, InferRequest, ToolDefinition};
    use mp_contracts::prompt_tier::PromptTierPlan;

    /// Recursively count `cache_control` keys anywhere in the body.
    fn count_cache_control(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::Object(map) => map
                .iter()
                .map(|(key, nested)| {
                    usize::from(key == "cache_control") + count_cache_control(nested)
                })
                .sum(),
            serde_json::Value::Array(items) => items.iter().map(count_cache_control).sum(),
            _ => 0,
        }
    }

    /// A tool definition whose schema is `schema_bytes` long, so a test can sit
    /// deliberately above or below a model's minimum cacheable length.
    fn padded_tool(schema_bytes: usize) -> ToolDefinition {
        let padding = "d".repeat(schema_bytes);
        ToolDefinition {
            name: "search_erp".to_owned(),
            description: "Search the ERP".to_owned(),
            parameters_json: format!(
                r#"{{"type":"object","properties":{{"q":{{"type":"string","description":"{padding}"}}}}}}"#
            ),
        }
    }

    /// A request big enough to clear the Sonnet-class 1024-token minimum on all
    /// three prefixes (tools, system, messages) — roughly 4 KiB each.
    fn large_request(model: &str) -> InferRequest {
        InferRequest {
            thinking_budget_tokens: 0,
            model: model.to_owned(),
            max_tokens: 1024,
            tools: vec![padded_tool(5_000)],
            tool_choice: "auto".to_owned(),
            messages: vec![
                ChatMessage {
                    compaction_summary: String::new(),
                    role: "system".to_owned(),
                    content: format!("You are Verevon. {}", "s".repeat(5_000)),
                    name: String::new(),
                },
                ChatMessage {
                    compaction_summary: String::new(),
                    role: "user".to_owned(),
                    content: format!("Hva er på lager? {}", "u".repeat(5_000)),
                    name: String::new(),
                },
            ],
            zdr: false,
            ..Default::default()
        }
    }

    /// ⚠ COMPLIANCE: Zero Data Retention forbids prompt caching outright.
    ///
    /// Anthropic's prompt cache holds the marked prefix server-side for minutes.
    /// That is retention, so a ZDR request must carry NO `cache_control`
    /// anywhere in its body — not on the tools, not on the system prompt, not on
    /// any message — no matter how large and cacheable the prompt is. This
    /// mirrors `cache.rs`, which bypasses the local prompt cache on both read and
    /// write under ZDR. If this test fails, Verevon is leaking customer prompt
    /// content into a provider-side cache in violation of its ZDR contract; do
    /// not "fix" it by relaxing the assertion.
    #[test]
    fn zdr_request_must_not_send_cache_control_anywhere_in_the_body() {
        let mut req = large_request("claude-sonnet-4-6");
        req.zdr = true;

        let body = build_request_body(&req);
        let serialized = serde_json::to_string(&body).expect("serialize body");

        assert!(
            !serialized.contains("cache_control"),
            "ZDR VIOLATION: prompt-cache breakpoint sent to the provider: {serialized}"
        );
        assert_eq!(
            count_cache_control(&body),
            0,
            "ZDR VIOLATION: cache_control present in the request body"
        );
        // The unmarked shapes must also be the plain forms — a structured block
        // rewrite would signal that marking ran and was merely stripped.
        assert!(body["system"].is_string(), "ZDR body kept the plain system");
        assert!(
            body["messages"][0]["content"].is_string(),
            "ZDR body kept plain message content"
        );

        // Same prompt without the ZDR bit MUST cache — otherwise this test would
        // pass on a build where caching is broken for everyone.
        let mut non_zdr = req.clone();
        non_zdr.zdr = false;
        let non_zdr_body = build_request_body(&non_zdr);
        assert!(
            count_cache_control(&non_zdr_body) > 0,
            "control arm: a non-ZDR request of the same prompt must be cached"
        );
    }

    #[test]
    fn large_non_zdr_request_marks_tools_system_and_the_last_message() {
        let body = build_request_body(&large_request("claude-sonnet-4-6"));

        // Tool block: the marker hangs off the LAST tool definition, which caches
        // the whole `tools` array (first in Anthropic's cache hierarchy).
        assert_eq!(
            body["tools"][0]["cache_control"]["type"], "ephemeral",
            "tool definitions are byte-identical every round — must be cached"
        );
        // System prompt: rewritten to the structured block form so the marker has
        // somewhere to live.
        assert_eq!(body["system"][0]["type"], "text");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(body["system"][0]["text"]
            .as_str()
            .expect("system text")
            .starts_with("You are Verevon."));
        // Last message: the rolling breakpoint that makes round N+1 cheap.
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(count_cache_control(&body), 3);
    }

    #[test]
    fn breakpoint_count_never_exceeds_anthropics_limit_of_four() {
        // A fifth `cache_control` is a hard 400 that fails the user's whole turn.
        // Pile on tools and history and assert the ceiling holds.
        let mut req = large_request("claude-sonnet-4-6");
        req.tools = (0..12).map(|_| padded_tool(4_000)).collect();
        for i in 0..24 {
            req.messages.push(ChatMessage {
                compaction_summary: String::new(),
                role: if i % 2 == 0 { "assistant" } else { "user" }.to_owned(),
                content: format!("round {i} {}", "h".repeat(2_000)),
                name: String::new(),
            });
        }

        let body = build_request_body(&req);
        let placed = count_cache_control(&body);
        assert!(
            placed <= MAX_CACHE_BREAKPOINTS,
            "placed {placed} breakpoints, over Anthropic's limit of {MAX_CACHE_BREAKPOINTS}"
        );
        assert!(placed > 0, "a large tool loop must still be cached");
    }

    #[test]
    fn prompt_below_the_minimum_cacheable_length_is_not_marked() {
        // Marking a prefix Anthropic is too small to cache buys nothing: it is
        // silently ignored, so the breakpoint slot is simply wasted.
        let req = InferRequest {
            thinking_budget_tokens: 0,
            model: "claude-sonnet-4-6".to_owned(),
            max_tokens: 256,
            tools: vec![padded_tool(8)],
            tool_choice: "auto".to_owned(),
            messages: vec![
                ChatMessage {
                    compaction_summary: String::new(),
                    role: "system".to_owned(),
                    content: "You are Verevon.".to_owned(),
                    name: String::new(),
                },
                ChatMessage {
                    compaction_summary: String::new(),
                    role: "user".to_owned(),
                    content: "Hei".to_owned(),
                    name: String::new(),
                },
            ],
            ..Default::default()
        };

        let body = build_request_body(&req);
        assert_eq!(
            count_cache_control(&body),
            0,
            "a tiny prompt must not be marked: {body}"
        );
        // Unmarked blocks keep the original plain-string shapes untouched.
        assert_eq!(body["system"], "You are Verevon.");
        assert_eq!(body["messages"][0]["content"], "Hei");
    }

    #[test]
    fn haiku_class_floor_is_higher_so_a_mid_sized_prompt_caches_only_on_sonnet() {
        assert_eq!(min_cacheable_tokens("claude-haiku-4-5"), 4_096);
        assert_eq!(min_cacheable_tokens("claude-sonnet-4-6"), 1_024);
        assert_eq!(min_cacheable_tokens("claude-opus-4-5"), 4_096);
        // Unknown/future deployment names fall back to the Sonnet-class floor.
        assert_eq!(min_cacheable_tokens("some-future-claude"), 1_024);

        // ~6 KiB of prompt ≈ 1500 estimated tokens: over Sonnet's 1024 floor,
        // under Haiku 4.5's 4096. The same prompt must cache on one and not the other.
        let mid_sized = |model: &str| InferRequest {
            thinking_budget_tokens: 0,
            model: model.to_owned(),
            max_tokens: 512,
            messages: vec![
                ChatMessage {
                    compaction_summary: String::new(),
                    role: "system".to_owned(),
                    content: "y".repeat(6_000),
                    name: String::new(),
                },
                ChatMessage {
                    compaction_summary: String::new(),
                    role: "user".to_owned(),
                    content: "Hei".to_owned(),
                    name: String::new(),
                },
            ],
            ..Default::default()
        };

        assert!(
            count_cache_control(&build_request_body(&mid_sized("claude-sonnet-4-6"))) > 0,
            "6 KiB clears the Sonnet-class floor"
        );
        assert_eq!(
            count_cache_control(&build_request_body(&mid_sized("claude-haiku-4-5"))),
            0,
            "6 KiB is under the Haiku-class floor — marking it would be inert"
        );
    }

    fn system_message(content: String) -> ChatMessage {
        ChatMessage {
            compaction_summary: String::new(),
            role: "system".to_owned(),
            content,
            name: String::new(),
        }
    }

    /// A tiered prompt: a Stable and a Context system message, each big enough
    /// to clear the Sonnet-class floor together, then a per-turn (Volatile)
    /// system message and the user turn.
    fn tiered_request() -> InferRequest {
        InferRequest {
            model: "claude-sonnet-4-6".to_owned(),
            max_tokens: 1024,
            prompt_tiers: PromptTierPlan {
                stable: 1,
                context: 1,
            },
            messages: vec![
                system_message(format!("STABLE {}", "s".repeat(5_000))),
                system_message(format!("CONTEXT {}", "c".repeat(5_000))),
                system_message("VOLATILE Today is Tuesday.".to_owned()),
                ChatMessage {
                    compaction_summary: String::new(),
                    role: "user".to_owned(),
                    content: "Hva er på lager?".to_owned(),
                    name: String::new(),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn tiered_system_marks_stable_and_context_not_volatile() {
        let body = build_request_body_with_flags(&tiered_request(), false, true);
        let system = body["system"].as_array().expect("ordered system blocks");

        assert_eq!(system.len(), 3);
        assert!(system[0]["text"].as_str().expect("text").starts_with("STABLE"));
        assert!(system[1]["text"].as_str().expect("text").starts_with("CONTEXT"));
        assert!(system[2]["text"].as_str().expect("text").starts_with("VOLATILE"));
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(system[1]["cache_control"]["type"], "ephemeral");
        assert!(
            system[2].get("cache_control").is_none(),
            "the per-turn block changes every turn: marking it would write the cache for nothing"
        );
        // Stable, Context and the rolling last message.
        assert_eq!(count_cache_control(&body), 3);
    }

    #[test]
    fn tiered_system_never_exceeds_four_breakpoints() {
        let mut req = tiered_request();
        req.tools = (0..12).map(|_| padded_tool(4_000)).collect();
        for i in 0..24 {
            req.messages.push(ChatMessage {
                compaction_summary: String::new(),
                role: if i % 2 == 0 { "assistant" } else { "user" }.to_owned(),
                content: format!("round {i} {}", "h".repeat(2_000)),
                name: String::new(),
            });
        }

        let body = build_request_body_with_flags(&req, false, true);
        // Tools, Stable, Context and the last message: exactly the limit.
        assert_eq!(count_cache_control(&body), MAX_CACHE_BREAKPOINTS);
    }

    #[test]
    fn no_tiers_is_byte_identical_to_today() {
        let mut untiered = tiered_request();
        untiered.prompt_tiers = PromptTierPlan::default();
        let plain = build_request_body_with_flags(&untiered, false, false);

        // The switch on with no plan changes nothing.
        assert_eq!(build_request_body_with_flags(&untiered, false, true), plain);
        // A plan with the switch off changes nothing either.
        assert_eq!(
            build_request_body_with_flags(&tiered_request(), false, false),
            plain
        );
        assert!(plain["system"].is_array(), "the untiered marked form is unchanged");
        assert_eq!(plain["system"].as_array().expect("blocks").len(), 1);
    }

    #[test]
    fn a_plan_that_does_not_describe_the_request_is_ignored() {
        let untiered = {
            let mut req = tiered_request();
            req.prompt_tiers = PromptTierPlan::default();
            build_request_body_with_flags(&req, false, true)
        };

        // A counted message that is not a system message.
        let mut not_system = tiered_request();
        not_system.messages[1].role = "user".to_owned();
        let untiered_not_system = {
            let mut req = not_system.clone();
            req.prompt_tiers = PromptTierPlan::default();
            build_request_body_with_flags(&req, false, true)
        };
        assert_eq!(
            build_request_body_with_flags(&not_system, false, true),
            untiered_not_system
        );

        // Counts past the end of the message list.
        let mut too_many = tiered_request();
        too_many.prompt_tiers = PromptTierPlan {
            stable: usize::MAX,
            context: 1,
        };
        assert_eq!(build_request_body_with_flags(&too_many, false, true), untiered);
    }

    #[test]
    fn below_minimum_prefix_marks_nothing() {
        let mut req = tiered_request();
        req.messages[0].content = "STABLE small".to_owned();
        req.messages[1].content = "CONTEXT small".to_owned();

        let body = build_request_body_with_flags(&req, false, true);
        assert_eq!(count_cache_control(&body), 0, "{body}");
        // Still ordered blocks, just none of them marked.
        assert_eq!(body["system"].as_array().expect("blocks").len(), 3);
    }

    #[test]
    fn a_prefix_that_only_the_context_tier_completes_marks_only_that_block() {
        let mut req = tiered_request();
        // 3 KiB alone is under the 1024-token floor; with the context it is over.
        req.messages[0].content = format!("STABLE {}", "s".repeat(3_000));
        req.messages[1].content = format!("CONTEXT {}", "c".repeat(3_000));

        let body = build_request_body_with_flags(&req, false, true);
        assert!(body["system"][0].get("cache_control").is_none());
        assert_eq!(body["system"][1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn zdr_tiered_request_sends_no_cache_control_and_one_system_string() {
        let mut req = tiered_request();
        req.zdr = true;

        let body = build_request_body_with_flags(&req, false, true);
        let serialized = serde_json::to_string(&body).expect("serialize body");
        assert!(
            !serialized.contains("cache_control"),
            "ZDR VIOLATION: prompt-cache breakpoint sent to the provider: {serialized}"
        );
        let system = body["system"].as_str().expect("ZDR keeps the plain system string");
        assert!(system.starts_with("STABLE"));
        assert!(system.contains("CONTEXT") && system.contains("VOLATILE"));
    }

    #[test]
    fn the_tiered_switch_is_enabled_only_by_the_literal_true() {
        assert!(tiered_system_flag_enabled(Some("true")));
        for raw in [None, Some(""), Some("1"), Some("True"), Some("yes"), Some("false")] {
            assert!(!tiered_system_flag_enabled(raw), "{raw:?}");
        }
    }

    #[test]
    fn input_tokens_still_reports_the_whole_input_when_the_prefix_was_cached() {
        // Anthropic's `input_tokens` counts only what follows the last
        // breakpoint. Cost accounting reads it as the request total, so the cache
        // legs must be folded back in or caching would silently under-bill.
        let json = serde_json::json!({
            "content": [{ "type": "text", "text": "ok" }],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 120,
                "cache_read_input_tokens": 8_000,
                "cache_creation_input_tokens": 400,
                "output_tokens": 42
            }
        });

        let parsed = parse_response("req-1", &json);
        assert_eq!(parsed.input_tokens, 8_520);
        assert_eq!(parsed.output_tokens, 42);

        // An uncached response (no cache fields at all) is unchanged.
        let uncached = serde_json::json!({
            "content": [{ "type": "text", "text": "ok" }],
            "usage": { "input_tokens": 120, "output_tokens": 42 }
        });
        assert_eq!(parse_response("req-2", &uncached).input_tokens, 120);
    }

    #[test]
    fn streaming_usage_comes_from_message_start_and_message_delta_not_message_stop() {
        // Real Anthropic streaming shape: `message_stop` carries no `usage`
        // field at all — reading it from there (the historical bug) always
        // resolves to 0. `input_tokens` (plus any cache legs) lives on
        // `message_start.message.usage`; the final `output_tokens` lives on
        // `message_delta.usage`.
        let message_start = serde_json::json!({
            "type": "message_start",
            "message": {
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4-6",
                "usage": {
                    "input_tokens": 25,
                    "cache_read_input_tokens": 8_000,
                    "cache_creation_input_tokens": 0,
                    "output_tokens": 1
                }
            }
        });
        let message_delta = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null },
            "usage": { "output_tokens": 63 }
        });
        let message_stop = serde_json::json!({ "type": "message_stop" });

        let input_tokens = total_input_tokens(&message_start["message"]["usage"]);
        let output_tokens = message_delta["usage"]["output_tokens"].as_i64().unwrap();

        assert_eq!(input_tokens, 8_025);
        assert_eq!(output_tokens, 63);
        // `message_stop` itself has no usage to read — confirms the fields
        // must be carried forward from the earlier events instead.
        assert!(message_stop["usage"].is_null());
    }

    #[test]
    fn message_delta_stop_reason_reads_the_nested_delta_path() {
        // Shape verified against platform.claude.com's streaming/handling-
        // stop-reasons reference: nested under `delta`, present ONLY on
        // `message_delta` -- `message_start` and `message_stop` never carry
        // it at all.
        let message_delta = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": "max_tokens", "stop_sequence": null },
            "usage": { "output_tokens": 63 }
        });
        assert_eq!(
            anthropic_message_delta_stop_reason(&message_delta),
            Some("max_tokens")
        );

        let message_stop = serde_json::json!({ "type": "message_stop" });
        assert_eq!(anthropic_message_delta_stop_reason(&message_stop), None);
    }
}

#[cfg(test)]
mod flavor_tests {
    use super::{is_cheap_claude, AnthropicProvider, ProviderRouter};

    #[test]
    fn direct_flavor_uses_first_party_messages_url() {
        let p = AnthropicProvider::new("k").expect("direct provider");
        assert_eq!(p.messages_url(), "https://api.anthropic.com/v1/messages");
        assert_eq!(p.provider_name(), "anthropic");
    }

    #[test]
    fn azure_flavor_appends_anthropic_messages_route() {
        let p = AnthropicProvider::new_azure(
            "k",
            "https://cloude-ai-resource.services.ai.azure.com/",
            vec!["claude-haiku-4-5".to_owned()],
        )
        .expect("azure provider");
        // Trailing slash on the endpoint must not double up.
        assert_eq!(
            p.messages_url(),
            "https://cloude-ai-resource.services.ai.azure.com/anthropic/v1/messages",
        );
        assert_eq!(p.provider_name(), "azure-anthropic");
    }

    #[test]
    fn azure_list_models_uses_configured_deployments_with_cheap_flag() {
        let p = AnthropicProvider::new_azure(
            "k",
            "https://cloude-ai-resource.services.ai.azure.com",
            vec!["claude-haiku-4-5".to_owned(), "claude-opus-4-8".to_owned()],
        )
        .expect("azure provider");
        let models = p.list_models();
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|m| m.provider == "azure-anthropic"));
        let haiku = models.iter().find(|m| m.id == "claude-haiku-4-5").unwrap();
        assert!(haiku.cheap, "haiku is the economy tier");
        let opus = models.iter().find(|m| m.id == "claude-opus-4-8").unwrap();
        assert!(!opus.cheap, "opus is not cheap");
    }

    #[test]
    fn empty_azure_key_or_endpoint_is_rejected() {
        assert!(AnthropicProvider::new_azure("", "https://x", vec![]).is_err());
        assert!(AnthropicProvider::new_azure("k", "   ", vec![]).is_err());
    }

    #[test]
    fn cheap_classifier_matches_only_haiku() {
        assert!(is_cheap_claude("claude-haiku-4-5"));
        assert!(is_cheap_claude("claude-3-5-haiku-20241022"));
        assert!(!is_cheap_claude("claude-opus-4-8"));
        assert!(!is_cheap_claude("claude-sonnet-4-6"));
    }
}

#[cfg(test)]
mod zdr_capability_tests {
    use std::sync::Arc;

    use super::{AnthropicProvider, ProviderRouter as _};
    use crate::provider::zdr::{RawAttestation, ZdrAttestation};

    fn attestation() -> Arc<ZdrAttestation> {
        let mut raw = RawAttestation {
            resource_id: "/subscriptions/abc/rg/eu/foundry-claude".to_owned(),
            approval_ref: "MAM-2026-0043".to_owned(),
            effective_date: "2026-06-01".to_owned(),
            review_by: "2027-06-01".to_owned(),
            reviewer: "ima@aquatiq.com".to_owned(),
            digest: String::new(),
        };
        // Use the shared canonical digest rather than recomputing it here: a second
        // implementation of the canonical form is a second thing that can drift.
        raw.digest = crate::provider::zdr::canonical_digest(&raw);
        Arc::new(
            ZdrAttestation::validate(
                "AZURE_ANTHROPIC",
                &raw,
                chrono::NaiveDate::from_ymd_opt(2026, 8, 17).expect("static date"),
            )
            .expect("fixture attestation must validate"),
        )
    }

    /// The regression this change exists for: `capabilities()` previously
    /// hardcoded `supports_zdr: false` with no builder, so the fallback chain
    /// skipped *both* Claude routes for every ZDR-flagged request and a chain
    /// with no other ZDR-capable provider returned `ZdrUnavailable`. Nothing
    /// distinguished "Claude cannot do ZDR" from "Claude was never asked".
    #[test]
    fn attested_azure_foundry_claude_advertises_zdr() {
        let provider = AnthropicProvider::new_azure(
            "k",
            "https://res.services.ai.azure.com",
            vec!["claude-sonnet-4-6".to_owned()],
        )
        .expect("azure provider")
        .with_zdr_attestation(Some(attestation()));
        assert!(
            provider.capabilities().supports_zdr,
            "an attested Foundry Claude resource must be eligible for ZDR traffic"
        );
    }

    /// A running process re-checks the date: an attestation that was valid at
    /// boot stops granting ZDR once its review-by date has passed, without a
    /// restart.
    #[test]
    fn review_by_passes_while_running_stops_granting() {
        use crate::provider::zdr::canonical_digest;
        let mut raw = RawAttestation {
            resource_id: "/subscriptions/abc/rg/eu/foundry-claude".to_owned(),
            approval_ref: "MAM-2020-0002".to_owned(),
            effective_date: "2020-01-01".to_owned(),
            review_by: "2020-06-01".to_owned(),
            reviewer: "ima@aquatiq.com".to_owned(),
            digest: String::new(),
        };
        raw.digest = canonical_digest(&raw);
        let lapsed = ZdrAttestation::validate(
            "AZURE_ANTHROPIC",
            &raw,
            chrono::NaiveDate::from_ymd_opt(2020, 3, 1).expect("static date"),
        )
        .expect("in force on the boot date");
        let provider = AnthropicProvider::new_azure(
            "k",
            "https://res.services.ai.azure.com",
            vec!["claude-sonnet-4-6".to_owned()],
        )
        .expect("azure provider")
        .with_zdr_attestation(Some(Arc::new(lapsed)));
        assert!(
            !provider.capabilities().supports_zdr,
            "an attestation past its review-by date must not advertise ZDR"
        );
    }

    /// Deny-by-default still holds: no attestation, no ZDR. Geography, transport
    /// encryption and a caller's `zdr` bit are not evidence of Anthropic's
    /// retention behavior on this resource.
    #[test]
    fn unattested_azure_foundry_claude_does_not_advertise_zdr() {
        let provider = AnthropicProvider::new_azure(
            "k",
            "https://res.services.ai.azure.com",
            vec!["claude-sonnet-4-6".to_owned()],
        )
        .expect("azure provider");
        assert!(
            !provider.capabilities().supports_zdr,
            "an unattested resource must never advertise ZDR"
        );
    }

    /// The direct `api.anthropic.com` route is never promoted implicitly — an
    /// attestation has to be handed to it deliberately, and the default is off.
    #[test]
    fn direct_anthropic_defaults_to_no_zdr() {
        let provider = AnthropicProvider::new("k").expect("direct provider");
        assert!(!provider.capabilities().supports_zdr);
    }

    /// `zdr` participates in the advertised feature flags the SPA gates on, so a
    /// newly attested resource must surface there too rather than only in the
    /// routing decision.
    #[test]
    fn attested_provider_advertises_the_zdr_feature_flag() {
        let provider = AnthropicProvider::new_azure(
            "k",
            "https://res.services.ai.azure.com",
            vec!["claude-sonnet-4-6".to_owned()],
        )
        .expect("azure provider")
        .with_zdr_attestation(Some(attestation()));
        assert!(
            provider
                .capabilities()
                .feature_flags()
                .contains(&"zdr".to_owned()),
            "an attested provider must advertise the zdr feature family"
        );
    }
}

#[cfg(test)]
mod extended_thinking_tests {
    use super::{resolve_thinking_budget, split_content_block_delta, MIN_THINKING_BUDGET_TOKENS};

    #[test]
    fn no_budget_requested_omits_the_parameter() {
        assert_eq!(resolve_thinking_budget("claude-opus-4-8", 0, 8192), None);
        assert_eq!(resolve_thinking_budget("claude-opus-4-8", -1, 8192), None);
    }

    /// Sending `thinking` to a model that predates it is a 400, so those models
    /// must silently omit it rather than fail the turn.
    #[test]
    fn models_that_predate_thinking_omit_it() {
        for model in [
            "claude-3-opus-20240229",
            "claude-3-5-sonnet-20241022",
            "claude-3.5-haiku",
            "claude-2.1",
            "claude-instant-1.2",
        ] {
            assert_eq!(
                resolve_thinking_budget(model, 2048, 8192),
                None,
                "{model} rejects the thinking parameter"
            );
        }
    }

    /// The exclusion list must not accidentally veto current or future models —
    /// that is the failure direction it was chosen to avoid.
    #[test]
    fn current_and_future_models_are_allowed_to_think() {
        for model in [
            "claude-opus-4-8",
            "claude-sonnet-4-5",
            "claude-3-7-sonnet-20250219",
            "claude-opus-5",
        ] {
            assert_eq!(
                resolve_thinking_budget(model, 2048, 8192),
                Some(2048),
                "{model} should be allowed extended thinking"
            );
        }
    }

    #[test]
    fn a_budget_below_the_provider_floor_is_omitted_not_clamped() {
        // Clamping up would spend tokens the caller did not ask for; clamping
        // down is impossible. Omitting keeps the turn working.
        assert_eq!(
            resolve_thinking_budget("claude-opus-4-8", MIN_THINKING_BUDGET_TOKENS - 1, 8192),
            None
        );
        assert_eq!(
            resolve_thinking_budget("claude-opus-4-8", MIN_THINKING_BUDGET_TOKENS, 8192),
            Some(MIN_THINKING_BUDGET_TOKENS)
        );
    }

    /// The budget is drawn FROM max_tokens, so one that does not leave room for
    /// an answer must be dropped — otherwise the provider 400s, or worse the
    /// model reasons well and has no room to reply.
    #[test]
    fn a_budget_that_leaves_no_room_for_an_answer_is_omitted() {
        assert_eq!(resolve_thinking_budget("claude-opus-4-8", 8192, 8192), None);
        assert_eq!(resolve_thinking_budget("claude-opus-4-8", 8192, 4096), None);
        assert_eq!(
            resolve_thinking_budget("claude-opus-4-8", 4096, 8192),
            Some(4096)
        );
    }

    /// The bug the split fixes: reading `delta.text` unconditionally turned
    /// every thinking delta into an empty answer chunk.
    #[test]
    fn a_thinking_delta_never_becomes_answer_text() {
        let delta = serde_json::json!({ "type": "thinking_delta", "thinking": "let me check" });
        let (answer, reasoning) = split_content_block_delta(&delta);
        assert_eq!(answer, "", "reasoning must never land in the answer");
        assert_eq!(reasoning, "let me check");
    }

    #[test]
    fn a_text_delta_stays_answer_text() {
        let delta = serde_json::json!({ "type": "text_delta", "text": "Bergen is rainy" });
        assert_eq!(
            split_content_block_delta(&delta),
            ("Bergen is rainy".to_owned(), String::new())
        );
    }

    /// Every pre-thinking stream sent deltas with no `type`. Defaulting the
    /// other way would drop all answer text.
    #[test]
    fn an_untyped_delta_defaults_to_answer_text() {
        let delta = serde_json::json!({ "text": "hello" });
        assert_eq!(
            split_content_block_delta(&delta),
            ("hello".to_owned(), String::new())
        );
    }

    /// Signatures and tool-argument fragments are neither answer nor reasoning.
    #[test]
    fn non_readable_deltas_produce_nothing() {
        for delta in [
            serde_json::json!({ "type": "signature_delta", "signature": "abc123" }),
            serde_json::json!({ "type": "input_json_delta", "partial_json": "{\"a\":" }),
        ] {
            assert_eq!(
                split_content_block_delta(&delta),
                (String::new(), String::new()),
                "{delta} is not readable content"
            );
        }
    }
}

#[cfg(test)]
mod streaming_cache_telemetry_tests {
    //! Cache-token telemetry on the STREAMING path (`infer_stream`), the gap
    //! a prior audit found: `parse_response` (unary) has logged the cache
    //! legs for a while, but until now nothing here reported them on the
    //! wire, and `InferChunk` had no field to carry them even if it had.
    //! These exercise the real HTTP/SSE code path end to end (not just the
    //! `total_input_tokens` helper already covered above), against a fixture
    //! stream shaped exactly like `prompt_cache_tests`'s
    //! `streaming_usage_comes_from_message_start_and_message_delta_not_message_stop`.
    use super::{AnthropicProvider, ProviderRouter};
    use crate::provider::{ChatMessage, InferRequest};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    /// A scripted Anthropic SSE stream carrying `cache_read_input_tokens` and
    /// `cache_creation_input_tokens` on `message_start.message.usage`, ending
    /// normally via `message_stop`.
    fn cached_sse_body() -> String {
        concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet-4-6\",",
            "\"usage\":{\"input_tokens\":25,\"cache_read_input_tokens\":8000,",
            "\"cache_creation_input_tokens\":400,\"output_tokens\":1}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,",
            "\"delta\":{\"type\":\"text_delta\",\"text\":\"Hei\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},",
            "\"usage\":{\"output_tokens\":12}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        )
        .to_owned()
    }

    fn sample_request() -> InferRequest {
        InferRequest {
            request_id: "req-cache-1".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            max_tokens: 512,
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "Hei".to_owned(),
                name: String::new(),
            }],
            ..InferRequest::default()
        }
    }

    #[tokio::test]
    async fn infer_stream_carries_cache_legs_on_the_final_chunk() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/anthropic/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(cached_sse_body(), "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = AnthropicProvider::new_azure("key", server.uri(), vec![])
            .expect("azure-flavored provider for the mock endpoint");

        let mut stream = provider
            .infer_stream(&sample_request())
            .await
            .expect("stream starts");

        // Intermediate chunk: cache legs are exactly 0, not carried early.
        let delta_chunk = stream.recv().await.expect("delta chunk");
        assert!(!delta_chunk.done);
        assert_eq!(delta_chunk.cache_read_input_tokens, 0);
        assert_eq!(delta_chunk.cache_creation_input_tokens, 0);

        // Final chunk: the cache legs captured off `message_start` arrive
        // here, same rule as `input_tokens`/`output_tokens`.
        let final_chunk = stream.recv().await.expect("final chunk");
        assert!(final_chunk.done);
        assert_eq!(final_chunk.input_tokens, 8_425); // 25 + 8_000 + 400
        assert_eq!(final_chunk.output_tokens, 12);
        assert_eq!(final_chunk.cache_read_input_tokens, 8_000);
        assert_eq!(final_chunk.cache_creation_input_tokens, 400);
        assert_eq!(final_chunk.stop_reason, "end_turn");
    }

    /// An uncached stream (no cache fields in `usage` at all) must report
    /// exact zeros, not carry over stale state from a previous call.
    #[tokio::test]
    async fn infer_stream_reports_zero_cache_legs_when_uncached() {
        let server = MockServer::start().await;
        let body = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_2\",\"model\":\"claude-sonnet-4-6\",",
            "\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},",
            "\"usage\":{\"output_tokens\":9}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/anthropic/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;

        let provider = AnthropicProvider::new_azure("key", server.uri(), vec![])
            .expect("azure-flavored provider for the mock endpoint");

        let mut stream = provider
            .infer_stream(&sample_request())
            .await
            .expect("stream starts");
        let final_chunk = stream.recv().await.expect("final chunk");
        assert!(final_chunk.done);
        assert_eq!(final_chunk.input_tokens, 25);
        assert_eq!(final_chunk.cache_read_input_tokens, 0);
        assert_eq!(final_chunk.cache_creation_input_tokens, 0);
    }
}

#[cfg(test)]
mod native_context_management_tests {
    //! The native-compaction migration's inference-core half: every
    //! Anthropic-provider request carries the `context_management` block plus
    //! the `anthropic-beta` header (the model-gateway half — skipping the
    //! hand-rolled `clear_stale_tool_results` / head-summary compaction for an
    //! Anthropic-family model — lives in
    //! `model-gateway::compaction::{should_run_local_compaction,
    //! clear_stale_tool_results_unless_native}` and is tested there).
    use super::{
        applied_context_edits, build_request_body, context_management_beta_header,
        context_management_flag_enabled, parse_response, AnthropicProvider, ProviderRouter,
        COMPACTION_BETA, CONTEXT_MANAGEMENT_BETA,
    };
    use crate::provider::{ChatMessage, InferRequest};
    use wiremock::{
        matchers::{headers, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn sample_request() -> InferRequest {
        InferRequest {
            request_id: "req-ctx-mgmt-1".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            max_tokens: 512,
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "Hei".to_owned(),
                name: String::new(),
            }],
            ..InferRequest::default()
        }
    }

    /// The parser is pure precisely so this never has to touch `std::env`
    /// (racy under Rust's default parallel test runner) to exercise both
    /// branches.
    #[test]
    fn the_flag_defaults_on_and_only_a_recognized_falsy_value_turns_it_off() {
        assert!(context_management_flag_enabled(None));
        assert!(context_management_flag_enabled(Some("1")));
        assert!(context_management_flag_enabled(Some("anything-else")));
        for falsy in ["0", "false", "off", "no", "  off  "] {
            assert!(
                !context_management_flag_enabled(Some(falsy)),
                "{falsy:?} must disable the native path"
            );
        }
    }

    /// **Test 1 (part A):** a request this provider builds carries the native
    /// `context_management.edits[]` block with both strategies, on by
    /// default — no per-call opt-in from model-gateway required.
    #[test]
    fn build_request_body_carries_both_native_context_management_edits() {
        let body = build_request_body(&sample_request());
        let edits = body["context_management"]["edits"]
            .as_array()
            .expect("edits array present by default");
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0]["type"], "clear_tool_uses_20250919");
        assert_eq!(edits[0]["trigger"]["type"], "input_tokens");
        assert_eq!(edits[0]["keep"]["value"], 3);
        assert_eq!(edits[1]["type"], "compact_20260112");
        // Anthropic 400s a `compact` trigger below 50k tokens; guard the
        // literal this provider actually sends against ever regressing under
        // that floor.
        assert!(edits[1]["trigger"]["value"].as_u64().unwrap() >= 50_000);
        assert!(edits[1]["instructions"].as_str().is_some());
    }

    #[test]
    fn the_combined_beta_header_names_both_features_for_a_compaction_capable_model() {
        let header = context_management_beta_header("claude-sonnet-4-6");
        assert!(header.contains(CONTEXT_MANAGEMENT_BETA));
        assert!(header.contains(COMPACTION_BETA));
        assert_eq!(header, "context-management-2025-06-27,compact-2026-01-12");
    }

    /// **Live-verified regression guard.** Tested directly against a real
    /// Azure Foundry Claude deployment (Sept 2026): `claude-haiku-4-5` — the
    /// default model this provider substitutes for every unspecified-model
    /// request (`DEFAULT_AZURE_ANTHROPIC_MODEL`) — 400s outright when a
    /// request carries the `compact_20260112` edit, naming the model and the
    /// strategy in its error. `clear_tool_uses_20250919` showed no such
    /// restriction on any tier tested (haiku included). Both this and the
    /// body-shape test below exist so a body/header carrying `compact` for
    /// Haiku can never ship again unnoticed — that combination would 400 the
    /// DEFAULT route of this provider on every over-budget turn.
    #[test]
    fn haiku_omits_the_compaction_strategy_it_does_not_support() {
        let header = context_management_beta_header("claude-haiku-4-5");
        assert_eq!(header, CONTEXT_MANAGEMENT_BETA);
        assert!(!header.contains(COMPACTION_BETA));

        let mut req = sample_request();
        req.model = "claude-haiku-4-5".to_owned();
        let body = build_request_body(&req);
        let edits = body["context_management"]["edits"]
            .as_array()
            .expect("clear_tool_uses is still offered to haiku");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["type"], "clear_tool_uses_20250919");
        assert!(
            edits.iter().all(|edit| edit["type"] != "compact_20260112"),
            "haiku must never be sent the strategy it 400s on"
        );
    }

    /// **Test 1 (part B):** the header is not just built but actually placed
    /// on the wire, for BOTH the direct and Azure-Foundry flavors of a call
    /// this provider makes — a wiremock header matcher, not just a body
    /// assertion, so a future refactor that builds the right body but forgets
    /// to attach the header still fails this test.
    #[tokio::test]
    async fn infer_sends_the_native_context_management_beta_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/anthropic/v1/messages"))
            // wiremock's header matcher compares against a comma-joined
            // header by splitting it back into individual values, mirroring
            // HTTP's own "repeated header == comma-joined header" semantics
            // -- so the two betas are asserted as two separate values here,
            // exactly as `context_management_beta_header`'s own unit test
            // pins the single wire string they come from.
            .and(headers(
                "anthropic-beta",
                vec![CONTEXT_MANAGEMENT_BETA, COMPACTION_BETA],
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "content": [{ "type": "text", "text": "hei" }],
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": { "input_tokens": 10, "output_tokens": 2 }
            })))
            .mount(&server)
            .await;

        let provider = AnthropicProvider::new_azure("key", server.uri(), vec![])
            .expect("azure-flavored provider for the mock endpoint");

        // The mock only matches a request carrying the exact header; a
        // missing/garbled one falls through to wiremock's default 404 and
        // this call returns an error instead of the scripted 200.
        provider
            .infer(&sample_request())
            .await
            .expect("the beta header must have reached the wire for the mock to match");
    }

    #[tokio::test]
    async fn infer_stream_sends_the_native_context_management_beta_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/anthropic/v1/messages"))
            .and(headers(
                "anthropic-beta",
                vec![CONTEXT_MANAGEMENT_BETA, COMPACTION_BETA],
            ))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                concat!(
                    "event: message_start\n",
                    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",",
                    "\"model\":\"claude-sonnet-4-6\",\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n",
                    "event: message_stop\n",
                    "data: {\"type\":\"message_stop\"}\n\n"
                ),
                "text/event-stream",
            ))
            .mount(&server)
            .await;

        let provider = AnthropicProvider::new_azure("key", server.uri(), vec![])
            .expect("azure-flavored provider for the mock endpoint");

        let mut stream = provider
            .infer_stream(&sample_request())
            .await
            .expect("the beta header must have reached the wire for the mock to match");
        let final_chunk = stream.recv().await.expect("final chunk");
        assert!(final_chunk.done);
    }

    /// **Test 3:** the pre-existing cache-token telemetry (previous stage)
    /// stays populated on a response that ALSO carries native
    /// `context_management.applied_edits` — proving the new parsing this
    /// migration adds does not regress the cache-leg parsing it sits beside,
    /// on the exact response shape a real compacted/cleared turn produces.
    #[test]
    fn cache_token_telemetry_is_populated_alongside_applied_native_edits() {
        let json = serde_json::json!({
            "content": [{ "type": "text", "text": "ok" }],
            "model": "claude-sonnet-4-6",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 120,
                "cache_read_input_tokens": 8_000,
                "cache_creation_input_tokens": 400,
                "output_tokens": 42
            },
            "context_management": {
                "applied_edits": [
                    {
                        "type": "clear_tool_uses_20250919",
                        "cleared_tool_uses": 8,
                        "cleared_input_tokens": 50_000
                    },
                    {
                        "type": "compact_20260112",
                        "cleared_tool_uses": 0,
                        "cleared_input_tokens": 120_000
                    }
                ]
            }
        });

        let parsed = parse_response("req-1", &json);
        // Cache-token telemetry (previous stage): still correct, unaffected
        // by the sibling `context_management` key.
        assert_eq!(parsed.input_tokens, 8_520);
        assert_eq!(parsed.output_tokens, 42);

        // This migration's own telemetry: both applied edits are readable.
        let edits = applied_context_edits(&json);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0], ("clear_tool_uses_20250919".to_owned(), 8, 50_000));
        assert_eq!(edits[1], ("compact_20260112".to_owned(), 0, 120_000));
    }

    /// An ordinary turn that never triggered either strategy must report no
    /// applied edits — not a default/placeholder entry that would read as a
    /// phantom compaction in a log line.
    #[test]
    fn no_applied_edits_when_context_management_is_absent() {
        let json = serde_json::json!({
            "content": [{ "type": "text", "text": "ok" }],
            "usage": { "input_tokens": 10, "output_tokens": 2 }
        });
        assert!(applied_context_edits(&json).is_empty());
    }
}

#[cfg(test)]
mod ladder_wire_tests {
    use super::{
        adaptive_thinking_flag_enabled, advertised_effort_levels, apply_thinking,
        build_request_body, resolve_thinking_budget,
    };
    use crate::provider::effort::Level;
    use crate::provider::InferRequest;

    fn request(model: &str, effort: Option<Level>, legacy_budget: i32) -> InferRequest {
        InferRequest {
            model: model.to_owned(),
            max_tokens: 4096,
            reasoning_effort: effort,
            thinking_budget_tokens: legacy_budget,
            ..Default::default()
        }
    }

    fn ladder_body(req: &InferRequest) -> serde_json::Value {
        let mut body = serde_json::json!({ "model": req.model, "max_tokens": req.max_tokens });
        apply_thinking(&mut body, req, true);
        body
    }

    #[test]
    fn the_switch_is_off_unless_the_literal_true() {
        assert!(!adaptive_thinking_flag_enabled(None));
        for raw in ["", "1", "TRUE", "True", "yes", "on", " true", "false"] {
            assert!(!adaptive_thinking_flag_enabled(Some(raw)), "{raw:?}");
        }
        assert!(adaptive_thinking_flag_enabled(Some("true")));
    }

    #[test]
    fn switch_off_keeps_the_legacy_budget_block() {
        // The process environment does not set the switch, so the public
        // builder takes the legacy path.
        let body = build_request_body(&request("claude-opus-4-8", Some(Level::High), 2048));
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 2048);
        assert_eq!(body["max_tokens"], 4096);
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn adaptive_models_send_adaptive_plus_effort_not_budget() {
        let body = ladder_body(&request("claude-opus-4-8", Some(Level::High), 0));
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "high");
        assert!(body["thinking"].get("budget_tokens").is_none());
    }

    #[test]
    fn claude_unknown_future_model_defaults_to_adaptive() {
        for model in ["claude-opus-5", "claude-fable-9", "claude-mythos"] {
            let body = ladder_body(&request(model, Some(Level::Medium), 0));
            assert_eq!(body["thinking"]["type"], "adaptive", "{model}");
            assert_eq!(body["output_config"]["effort"], "medium", "{model}");
        }
    }

    #[test]
    fn budget_class_unchanged_for_sonnet_4_5() {
        for (level, legacy) in [(Level::Low, 1024), (Level::Medium, 2048), (Level::High, 4096)] {
            let body = ladder_body(&request("claude-sonnet-4-5", Some(level), 0));
            assert_eq!(body["thinking"]["type"], "enabled");
            assert_eq!(body["thinking"]["budget_tokens"], legacy);
            assert!(body.get("output_config").is_none());
            // The ladder's budget is exactly what the legacy path sent for the
            // same request when the gateway spoke budgets.
            assert_eq!(
                resolve_thinking_budget("claude-sonnet-4-5", legacy, 32_000),
                Some(legacy)
            );
        }
    }

    #[test]
    fn legacy_budget_requests_map_onto_the_ladder() {
        let body = ladder_body(&request("claude-sonnet-4-5", None, 1024));
        assert_eq!(body["thinking"]["budget_tokens"], 1024);
        let body = ladder_body(&request("claude-opus-4-8", None, 4096));
        assert_eq!(body["output_config"]["effort"], "high");
    }

    #[test]
    fn standard_omits_thinking_and_output_config() {
        for model in ["claude-opus-4-8", "claude-sonnet-4-5"] {
            let body = ladder_body(&request(model, None, 0));
            assert!(body.get("thinking").is_none(), "{model}");
            assert!(body.get("output_config").is_none(), "{model}");
            assert_eq!(body["max_tokens"], 4096, "{model}: no level, no raise");
        }
    }

    #[test]
    fn models_without_effort_control_send_nothing() {
        let body = ladder_body(&request("claude-3-5-haiku-20241022", Some(Level::High), 0));
        assert!(body.get("thinking").is_none());
        assert!(body.get("output_config").is_none());
        assert_eq!(body["max_tokens"], 4096);
    }

    #[test]
    fn summarized_display_is_requested() {
        let body = ladder_body(&request("claude-opus-4-8", Some(Level::Low), 0));
        assert_eq!(body["thinking"]["display"], "summarized");
    }

    #[test]
    fn max_tokens_includes_the_budget() {
        let body = ladder_body(&request("claude-sonnet-4-5", Some(Level::High), 0));
        let budget = body["thinking"]["budget_tokens"].as_i64().unwrap();
        let max_tokens = body["max_tokens"].as_i64().unwrap();
        assert_eq!(
            max_tokens,
            4096 + budget,
            "the answer ceiling is kept on top of the budget"
        );
        assert!(max_tokens > budget);
    }

    #[test]
    fn the_raise_is_capped_but_never_lowers_the_callers_ceiling() {
        let mut req = request("claude-opus-4-8", Some(Level::Max), 0);
        req.max_tokens = 30_000;
        assert_eq!(ladder_body(&req)["max_tokens"], 32_768);
        req.max_tokens = 40_000;
        assert_eq!(ladder_body(&req)["max_tokens"], 40_000);
    }

    #[test]
    fn a_requested_level_stronger_than_the_class_is_clamped() {
        // Budget-class models top out at High.
        let body = ladder_body(&request("claude-sonnet-4-5", Some(Level::Max), 0));
        assert_eq!(body["thinking"]["budget_tokens"], 4096);
    }

    #[test]
    fn foundry_and_direct_share_the_shape() {
        // The body builder takes no flavor, so both Claude routes render the
        // same shape, and the catalogue lists the same levels for a model.
        let req = request("claude-opus-4-8", Some(Level::XHigh), 0);
        assert_eq!(ladder_body(&req), ladder_body(&req.clone()));
        assert_eq!(ladder_body(&req)["output_config"]["effort"], "xhigh");
        assert_eq!(
            advertised_effort_levels("claude-opus-4-8", true),
            vec![Level::Low, Level::Medium, Level::High, Level::XHigh, Level::Max]
        );
    }

    #[test]
    fn the_catalogue_lists_levels_only_while_the_ladder_is_on() {
        assert!(advertised_effort_levels("claude-opus-4-8", false).is_empty());
        assert_eq!(
            advertised_effort_levels("claude-sonnet-4-5", true),
            vec![Level::Low, Level::Medium, Level::High]
        );
        assert!(advertised_effort_levels("claude-3-5-haiku-20241022", true).is_empty());
    }
}

#[cfg(test)]
mod claude_5_5_wire_tests {
    use super::{
        advertised_effort_levels, apply_thinking, build_request_body_with_flags,
        min_cacheable_tokens, successor_of_retired_model, with_current_model, AnthropicProvider,
        ProviderRouter, CLAUDE_HAIKU_5_5, CLAUDE_OPUS_5_5, CLAUDE_SONNET_5_5,
        DEFAULT_ANTHROPIC_MODEL,
    };
    use crate::provider::effort::Level;
    use crate::provider::{ChatMessage, InferRequest, ToolDefinition};

    const CLAUDE_5_5_MODELS: [&str; 3] = [CLAUDE_OPUS_5_5, CLAUDE_SONNET_5_5, CLAUDE_HAIKU_5_5];

    fn request(model: &str, effort: Option<Level>, legacy_budget: i32) -> InferRequest {
        InferRequest {
            model: model.to_owned(),
            max_tokens: 4096,
            reasoning_effort: effort,
            thinking_budget_tokens: legacy_budget,
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "Hei".to_owned(),
                name: String::new(),
            }],
            ..Default::default()
        }
    }

    /// The body as the adapter sends it, with the process switches at their defaults.
    fn body(req: &InferRequest) -> serde_json::Value {
        build_request_body_with_flags(req, false, false)
    }

    fn with_tool(model: &str, choice: &str) -> InferRequest {
        InferRequest {
            tools: vec![ToolDefinition {
                name: "get_weather".to_owned(),
                description: "Get the weather".to_owned(),
                parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
            }],
            tool_choice: choice.to_owned(),
            ..request(model, None, 0)
        }
    }

    #[test]
    fn each_5_5_model_is_sent_under_its_exact_id() {
        for model in CLAUDE_5_5_MODELS {
            assert_eq!(body(&request(model, None, 0))["model"], model);
        }
        assert_eq!(DEFAULT_ANTHROPIC_MODEL, CLAUDE_SONNET_5_5);
    }

    #[test]
    fn a_5_5_request_with_an_effort_is_adaptive_at_that_effort() {
        for model in CLAUDE_5_5_MODELS {
            let sent = body(&request(model, Some(Level::High), 0));
            assert_eq!(sent["thinking"]["type"], "adaptive", "{model}");
            assert_eq!(sent["thinking"]["display"], "summarized", "{model}");
            assert!(sent["thinking"].get("budget_tokens").is_none(), "{model}");
            assert_eq!(sent["output_config"]["effort"], "high", "{model}");
            // The high level's allowance is added to the caller's ceiling.
            assert_eq!(sent["max_tokens"], 4096 + 4096, "{model}");
        }
    }

    #[test]
    fn a_5_5_legacy_budget_is_read_as_a_level_never_sent_as_a_budget() {
        for model in CLAUDE_5_5_MODELS {
            let sent = body(&request(model, None, 2048));
            assert_eq!(sent["thinking"]["type"], "adaptive", "{model}");
            assert!(sent["thinking"].get("budget_tokens").is_none(), "{model}");
            assert_eq!(sent["output_config"]["effort"], "medium", "{model}");
        }
    }

    #[test]
    fn a_5_5_request_naming_no_level_runs_at_the_documented_default() {
        for (model, default, allowance) in [
            (CLAUDE_OPUS_5_5, "medium", 2048),
            (CLAUDE_SONNET_5_5, "high", 4096),
            (CLAUDE_HAIKU_5_5, "medium", 2048),
        ] {
            let sent = body(&request(model, None, 0));
            assert_eq!(sent["thinking"]["type"], "adaptive", "{model}");
            assert!(sent["thinking"].get("budget_tokens").is_none(), "{model}");
            assert_eq!(sent["output_config"]["effort"], default, "{model}");
            // The default level's allowance is added to the caller's ceiling, as for a named level.
            assert_eq!(sent["max_tokens"], 4096 + allowance, "{model}");
        }
    }

    #[test]
    fn every_level_reaches_a_5_5_model_and_minimal_rounds_up_to_low() {
        for model in CLAUDE_5_5_MODELS {
            for (asked, sent_as) in [
                (Level::Minimal, "low"),
                (Level::Low, "low"),
                (Level::Medium, "medium"),
                (Level::High, "high"),
                (Level::XHigh, "xhigh"),
                (Level::Max, "max"),
            ] {
                let sent = body(&request(model, Some(asked), 0));
                assert_eq!(sent["output_config"]["effort"], sent_as, "{model} asked {asked:?}");
            }
        }
    }

    #[test]
    fn the_adaptive_switch_has_no_say_over_the_5_5_wire() {
        for model in CLAUDE_5_5_MODELS {
            for req in [
                request(model, Some(Level::Low), 2048),
                request(model, None, 2048),
                request(model, None, 0),
            ] {
                let mut off = serde_json::json!({});
                let mut on = serde_json::json!({});
                apply_thinking(&mut off, &req, false);
                apply_thinking(&mut on, &req, true);
                assert_eq!(off, on, "{model}");
            }
        }
    }

    #[test]
    fn no_sampling_parameter_is_sent_to_a_5_5_model() {
        for model in CLAUDE_5_5_MODELS {
            let mut req = request(model, Some(Level::Low), 0);
            req.temperature = 0.7;
            let sent = body(&req);
            for key in ["temperature", "top_p", "top_k"] {
                assert!(sent.get(key).is_none(), "{model} sent {key}");
            }
        }
    }

    #[test]
    fn a_forced_tool_choice_is_sent_as_auto_on_opus_and_sonnet_5_5() {
        for model in [CLAUDE_OPUS_5_5, CLAUDE_SONNET_5_5] {
            for forced in ["required", "get_weather"] {
                assert_eq!(
                    body(&with_tool(model, forced))["tool_choice"]["type"],
                    "auto",
                    "{model} {forced}"
                );
            }
            assert_eq!(body(&with_tool(model, "auto"))["tool_choice"]["type"], "auto", "{model}");
            assert_eq!(body(&with_tool(model, "none"))["tool_choice"]["type"], "none", "{model}");
        }
        // Haiku 5.5 takes a forced choice, and so does an older model.
        assert_eq!(body(&with_tool(CLAUDE_HAIKU_5_5, "required"))["tool_choice"]["type"], "any");
        assert_eq!(
            body(&with_tool("claude-sonnet-4-6", "get_weather"))["tool_choice"]["name"],
            "get_weather"
        );
    }

    #[test]
    fn a_retired_model_id_is_sent_as_its_successor_on_the_adaptive_wire() {
        for (retired, successor) in [
            ("claude-opus-4-20250514", CLAUDE_OPUS_5_5),
            ("claude-sonnet-4-20250514", CLAUDE_SONNET_5_5),
            ("claude-3-5-haiku-20241022", CLAUDE_HAIKU_5_5),
        ] {
            let sent = body(&request(retired, Some(Level::Medium), 0));
            assert_eq!(sent["model"], successor, "{retired}");
            assert_eq!(sent["thinking"]["type"], "adaptive", "{retired}");
            assert_eq!(sent["output_config"]["effort"], "medium", "{retired}");
        }
    }

    #[test]
    fn the_successor_map_is_exact_and_leaves_every_other_id_alone() {
        assert_eq!(
            successor_of_retired_model(" Claude-Sonnet-4-20250514 "),
            Some(CLAUDE_SONNET_5_5)
        );
        for model in [
            "claude-sonnet-4-20250515",
            "claude-sonnet-4-6",
            "claude-opus-5-5",
            "claude-3-5-haiku-20241023",
            "",
        ] {
            assert_eq!(successor_of_retired_model(model), None, "{model:?}");
        }
        assert!(with_current_model(&request("claude-sonnet-4-6", None, 0)).is_none());
        let retired = request("claude-opus-4-20250514", None, 0);
        let mapped = with_current_model(&retired).expect("a retired id is mapped");
        assert_eq!(mapped.model, CLAUDE_OPUS_5_5);
        assert_eq!(mapped.messages.len(), 1);
        assert_eq!(mapped.messages[0].content, "Hei");
    }

    #[test]
    fn the_catalogue_offers_the_three_5_5_models_with_their_levels_and_limits() {
        let provider = AnthropicProvider::new("test-key").expect("a provider with a key");
        let models = provider.list_models();
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, [CLAUDE_SONNET_5_5, CLAUDE_OPUS_5_5, CLAUDE_HAIKU_5_5]);
        for model in &models {
            assert_eq!(
                model.effort_levels,
                vec![Level::Low, Level::Medium, Level::High, Level::XHigh, Level::Max],
                "{}",
                model.id
            );
        }
        assert!(models.iter().any(|model| model.id == CLAUDE_HAIKU_5_5 && model.cheap));
        assert_eq!(advertised_effort_levels(CLAUDE_OPUS_5_5, false).len(), 5);
        let caps = provider.capabilities();
        assert_eq!(caps.max_context_tokens, 1_000_000);
        assert_eq!(caps.max_output_tokens, 128_000);
    }

    #[test]
    fn the_cacheable_floor_follows_each_generations_published_minimum() {
        for model in [
            CLAUDE_OPUS_5_5,
            CLAUDE_SONNET_5_5,
            CLAUDE_HAIKU_5_5,
            "claude-opus-5",
            "claude-fable-5-1",
            "claude-mythos-5-1",
        ] {
            assert_eq!(min_cacheable_tokens(model), 512, "{model}");
        }
        assert_eq!(min_cacheable_tokens("claude-sonnet-5"), 1_024);
        assert_eq!(min_cacheable_tokens("claude-haiku-4-5"), 4_096);
        assert_eq!(min_cacheable_tokens("claude-3-5-haiku-20241022"), 2_048);
        assert_eq!(min_cacheable_tokens("claude-opus-4-6"), 4_096);
        assert_eq!(min_cacheable_tokens("claude-opus-4-7"), 2_048);
    }

    #[test]
    fn a_5_5_prompt_above_the_512_floor_is_marked_for_caching() {
        for model in CLAUDE_5_5_MODELS {
            let req = InferRequest {
                model: model.to_owned(),
                max_tokens: 256,
                messages: vec![
                    ChatMessage {
                        compaction_summary: String::new(),
                        role: "system".to_owned(),
                        content: "y".repeat(2_400),
                        name: String::new(),
                    },
                    ChatMessage {
                        compaction_summary: String::new(),
                        role: "user".to_owned(),
                        content: "Hei".to_owned(),
                        name: String::new(),
                    },
                ],
                ..Default::default()
            };
            let sent = body(&req);
            assert_eq!(sent["system"][0]["cache_control"]["type"], "ephemeral", "{model}");
        }
    }
}

#[cfg(test)]
mod effort_log_tests {
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    use super::{
        apply_thinking, build_request_body_with_flags, claude_5_5_default_effort, CLAUDE_HAIKU_5_5,
        CLAUDE_OPUS_5_5, CLAUDE_SONNET_5_5,
    };
    use crate::provider::effort::Level;
    use crate::provider::{ChatMessage, InferRequest};

    /// The level and fields of one logged event, with string values unquoted.
    type Logged = (tracing::Level, Vec<(String, String)>);

    #[derive(Default)]
    struct FieldCollector(Vec<(String, String)>);

    impl Visit for FieldCollector {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            let value = format!("{value:?}");
            let name = field.name().to_owned();
            self.0.push((name, value.trim_matches('"').to_owned()));
        }
    }

    struct Recorder(Arc<Mutex<Vec<Logged>>>);

    impl Subscriber for Recorder {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, event: &Event<'_>) {
            let mut fields = FieldCollector::default();
            event.record(&mut fields);
            let level = *event.metadata().level();
            self.0.lock().unwrap().push((level, fields.0));
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    /// Runs `build` under a fresh subscriber and returns what it built, with the
    /// events it logged. Callsite interest is cached process-wide, so a first
    /// registration that overlaps another test's subscriber can leave a stale
    /// "never" behind and drop an event. Each attempt installs its own subscriber,
    /// which rebuilds that cache, and the first attempt that logged anything wins.
    fn run_logged(build: impl Fn() -> serde_json::Value) -> (serde_json::Value, Vec<Logged>) {
        let mut last = None;
        for _ in 0..5 {
            let sink = Arc::new(Mutex::new(Vec::new()));
            let body = tracing::subscriber::with_default(Recorder(sink.clone()), || build());
            let events = sink.lock().unwrap().clone();
            if !events.is_empty() {
                return (body, events);
            }
            last = Some((body, events));
        }
        last.expect("the loop runs at least once")
    }

    fn field<'a>(event: &'a Logged, name: &str) -> &'a str {
        event
            .1
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value.as_str())
            .unwrap_or_else(|| panic!("no `{name}` field in {:?}", event.1))
    }

    /// The events that are the `effort_resolved` line.
    fn effort_lines(events: &[Logged]) -> Vec<&Logged> {
        events
            .iter()
            .filter(|event| field(event, "message") == "effort_resolved")
            .collect()
    }

    #[test]
    fn anthropic_ladder_logs_effort_resolved_once_per_body() {
        const MARKER: &str = "SECRETMARKER";
        let req = InferRequest {
            request_id: "req-effort-once".to_owned(),
            model: "claude-sonnet-4-5".to_owned(),
            max_tokens: 4096,
            reasoning_effort: Some(Level::High),
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: MARKER.to_owned(),
                name: String::new(),
            }],
            ..Default::default()
        };
        let (_, events) = run_logged(|| {
            let mut body = serde_json::json!({ "model": req.model, "max_tokens": req.max_tokens });
            apply_thinking(&mut body, &req, true);
            body
        });
        let lines = effort_lines(&events);
        assert_eq!(lines.len(), 1, "one line per body: {events:?}");
        let line = lines[0];
        assert_eq!(line.0, tracing::Level::INFO);
        assert_eq!(field(line, "request_id"), "req-effort-once");
        assert_eq!(field(line, "model"), "claude-sonnet-4-5");
        assert_eq!(field(line, "wire"), "AnthropicBudget");
        assert_eq!(field(line, "requested"), "high");
        assert_eq!(field(line, "applied"), "high");
        assert_eq!(field(line, "clamped"), "false");
        // The caller's 4096 plus the High allowance of 4096.
        assert_eq!(field(line, "max_tokens"), "8192");
        // Seven fields and the message, and no content.
        assert_eq!(line.1.len(), 8, "{:?}", line.1);
        let leaked = line.1.iter().any(|(_, value)| value.contains(MARKER));
        assert!(!leaked, "{:?}", line.1);
    }

    #[test]
    fn a_dropped_budget_logs_no_effort_resolved() {
        // At max_tokens 0 the raised ceiling is the High allowance, 4096, which is
        // the budget itself, so the no-room guard drops the thinking block.
        let req = InferRequest {
            model: "claude-sonnet-4-5".to_owned(),
            max_tokens: 0,
            reasoning_effort: Some(Level::High),
            ..Default::default()
        };
        let (body, events) = run_logged(|| {
            let mut body = serde_json::json!({ "model": req.model, "max_tokens": req.max_tokens });
            apply_thinking(&mut body, &req, true);
            body
        });
        assert!(body.get("thinking").is_none(), "{body}");
        assert_eq!(body["max_tokens"], 0, "a dropped budget raises nothing");
        // The guard's own warning shows the subscriber was listening, so the
        // missing level line is a real absence.
        let warned = events
            .iter()
            .any(|event| field(event, "message").contains("leaves no room"));
        assert!(warned, "{events:?}");
        assert!(effort_lines(&events).is_empty(), "{events:?}");
    }

    #[test]
    fn a_claude_5_5_request_logs_its_default_level() {
        const MARKER: &str = "SECRETMARKER";
        for model in [CLAUDE_OPUS_5_5, CLAUDE_SONNET_5_5, CLAUDE_HAIKU_5_5] {
            // No level and no legacy budget, so the model's documented default runs.
            let req = InferRequest {
                request_id: "req-5-5-default".to_owned(),
                model: model.to_owned(),
                max_tokens: 4096,
                messages: vec![ChatMessage {
                    compaction_summary: String::new(),
                    role: "user".to_owned(),
                    content: MARKER.to_owned(),
                    name: String::new(),
                }],
                ..Default::default()
            };
            let default = claude_5_5_default_effort(model).expect("a 5.5 model has a default");
            let (_, events) = run_logged(|| build_request_body_with_flags(&req, false, false));
            let lines = effort_lines(&events);
            assert_eq!(lines.len(), 1, "{model}: {events:?}");
            let line = lines[0];
            assert_eq!(field(line, "wire"), "AnthropicAdaptive", "{model}");
            assert_eq!(field(line, "requested"), default.as_str(), "{model}");
            assert_eq!(field(line, "applied"), default.as_str(), "{model}");
            assert_eq!(field(line, "clamped"), "false", "{model}");
            let leaked = line.1.iter().any(|(_, value)| value.contains(MARKER));
            assert!(!leaked, "{model}");
        }
    }
}
