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

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::zdr::ZdrAttestation;
use super::{InferChunk, InferRequest, InferResponse, ModelInfo, ProviderError, ProviderRouter};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Default Claude model used when a request leaves the model unspecified
/// ("Verevon Auto"). The fallback chain substitutes this when Anthropic is the
/// provider serving an unpinned request, so chat works against an
/// Anthropic-only deployment with no client- or operator-chosen model.
pub(crate) const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-20250514";

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
    if model.contains("haiku") {
        // Haiku-class: the economy tier carries the highest floor, so the same
        // prompt that caches on Sonnet may be too small to cache here.
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
    } else {
        // Sonnet-class (and Opus 4.8 / Opus 5, whose real floors are lower —
        // treating them as 1024 is the conservative direction).
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
    if let Some(system) = body
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

fn build_request_body(req: &InferRequest) -> serde_json::Value {
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
        .filter(|m| !m.role.eq_ignore_ascii_case("system"))
        .map(|m| {
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
    if let Some(budget) =
        resolve_thinking_budget(&req.model, req.thinking_budget_tokens, req.max_tokens)
    {
        body["thinking"] = serde_json::json!({
            "type": "enabled",
            "budget_tokens": budget,
        });
    }
    // `temperature` is deliberately omitted. Anthropic constrains it to [0, 1]
    // (vs OpenAI's [0, 2]) and the newest Claude models reject it outright
    // ("`temperature` is deprecated for this model" — e.g. claude-opus-4-8), so
    // forwarding a chat temperature 400s on those. Anthropic's own default
    // sampling is used instead; structured output is steered via the prompt
    // (see the response_format note above), not the temperature.
    if !system.is_empty() {
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
        let tools: Vec<serde_json::Value> = req
            .tools
            .iter()
            .map(|t| {
                let schema = super::tool_parameters(&t.name, &t.parameters_json);
                serde_json::json!({
                    "name": t.name,
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
        body["tool_choice"] = match tc {
            "auto" => serde_json::json!({ "type": "auto" }),
            "none" => serde_json::json!({ "type": "none" }),
            "required" => serde_json::json!({ "type": "any" }),
            name => serde_json::json!({ "type": "tool", "name": name }),
        };
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
    to_i32_or_max(usage["cache_read_input_tokens"].as_i64().unwrap_or(0))
}

/// Tokens newly written to Anthropic's prompt cache by this request (0 when
/// absent). See [`cache_read_input_tokens`] for the `usage` shape.
fn cache_creation_input_tokens(usage: &serde_json::Value) -> i32 {
    to_i32_or_max(usage["cache_creation_input_tokens"].as_i64().unwrap_or(0))
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
    to_i32_or_max(usage["input_tokens"].as_i64().unwrap_or(0))
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
    let output_tokens = to_i32_or_max(json["usage"]["output_tokens"].as_i64().unwrap_or(0));
    let tool_calls = parse_tool_calls(json);

    InferResponse {
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
    }
}

#[allow(clippy::too_many_lines)]
#[async_trait::async_trait]
impl ProviderRouter for AnthropicProvider {
    fn capabilities(&self) -> super::ProviderCapabilities {
        // Claude: tools, vision, extended thinking, streaming; 200k context.
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
            supports_zdr: self.zdr.is_some(),
            modalities: vec!["chat".to_owned(), "vision".to_owned()],
            max_context_tokens: 200_000,
            max_output_tokens: 8_192,
        }
    }

    /// Advertise the Claude chat catalog so `/v1/models` is populated (the SPA
    /// model picker reads this). Direct uses the first-party model ids; Azure
    /// Foundry uses the deployed Claude deployment names from config.
    fn list_models(&self) -> Vec<ModelInfo> {
        let provider = self.provider_name().to_owned();
        let ids: Vec<String> = match &self.flavor {
            AnthropicFlavor::Direct => [
                DEFAULT_ANTHROPIC_MODEL,
                "claude-opus-4-20250514",
                "claude-3-5-haiku-20241022",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            AnthropicFlavor::Azure { models, .. } => models.clone(),
        };
        // Venice-style per-model privacy disclosure, derived from this
        // provider's own declared residency + ZDR attestation.
        let caps = self.capabilities();
        let privacy_tier = super::PrivacyTier::classify(&caps);
        let residency_label = if caps.residency == super::Residency::Global {
            String::new()
        } else {
            caps.residency.as_str().to_owned()
        };
        ids.into_iter()
            .map(|id| {
                let cheap = is_cheap_claude(&id);
                ModelInfo {
                    id,
                    provider: provider.clone(),
                    modality: "chat".to_owned(),
                    streaming: true,
                    features: vec![
                        "tools".to_owned(),
                        "vision".to_owned(),
                        "reasoning".to_owned(),
                    ],
                    cheap,
                    privacy_tier,
                    residency_label: residency_label.clone(),
                }
            })
            .collect()
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        let body = build_request_body(req);

        let response = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
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
            return Err(crate::provider::overflow::classify_http_failure(
                status, &text,
            ));
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
        Ok(parse_response(&req.request_id, &json))
    }

    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        let mut body = build_request_body(req);
        body["stream"] = serde_json::Value::Bool(true);

        let response = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
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
            return Err(crate::provider::overflow::classify_http_failure(
                status, &text,
            ));
        }

        let request_id = req.request_id.clone();
        let model = req.model.clone();
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
            // Set from `message_delta.delta.stop_reason` once it arrives.
            // Stays empty if the loop exits without ever seeing one -- the
            // tail-chunk fallback below treats that as an incomplete stream,
            // not a natural completion.
            let mut stop_reason = String::new();

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
                                reasoning_delta: String::new(),
                                request_id: request_id.clone(),
                                delta: String::new(),
                                done: true,
                                model_used: model.clone(),
                                input_tokens,
                                output_tokens,
                                provider_used: String::new(),
                                residency: String::new(),
                                stop_reason: if stop_reason.is_empty() {
                                    "end_turn".to_owned()
                                } else {
                                    stop_reason.clone()
                                },
                            };
                            let _ = tx.send(final_chunk).await;
                            return;
                        }

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            let event_type = json["type"].as_str().unwrap_or("");

                            if event_type == "message_start" {
                                input_tokens = total_input_tokens(&json["message"]["usage"]);
                            } else if event_type == "message_delta" {
                                if let Some(v) = json["usage"]["output_tokens"].as_i64() {
                                    output_tokens = to_i32_or_max(v);
                                }
                                if let Some(reason) = anthropic_message_delta_stop_reason(&json) {
                                    stop_reason = reason.to_owned();
                                }
                            } else if event_type == "content_block_delta" {
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
                                    request_id: request_id.clone(),
                                    delta,
                                    done: false,
                                    model_used: model.clone(),
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    stop_reason: String::new(),
                                    reasoning_delta,
                                };
                                if tx.send(chunk).await.is_err() {
                                    return;
                                }
                            } else if event_type == "message_stop" {
                                let final_chunk = InferChunk {
                                    reasoning_delta: String::new(),
                                    request_id: request_id.clone(),
                                    delta: String::new(),
                                    done: true,
                                    model_used: model.clone(),
                                    input_tokens,
                                    output_tokens,
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    stop_reason: if stop_reason.is_empty() {
                                        "end_turn".to_owned()
                                    } else {
                                        stop_reason.clone()
                                    },
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
                reasoning_delta: String::new(),
                request_id,
                delta: String::new(),
                done: true,
                model_used: model,
                input_tokens,
                output_tokens,
                provider_used: String::new(),
                residency: String::new(),
                stop_reason: if stop_reason.is_empty() {
                    "stream_incomplete".to_owned()
                } else {
                    stop_reason
                },
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
            model: "claude-sonnet-4-20250514".to_owned(),
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
                    role: "system".to_owned(),
                    content: "You are Verevon.".to_owned(),
                    name: String::new(),
                },
                ChatMessage {
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
}

#[cfg(test)]
mod prompt_cache_tests {
    use super::{
        anthropic_message_delta_stop_reason, build_request_body, min_cacheable_tokens,
        parse_response, total_input_tokens, MAX_CACHE_BREAKPOINTS,
    };
    use crate::provider::{ChatMessage, InferRequest, ToolDefinition};

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
                    role: "system".to_owned(),
                    content: format!("You are Verevon. {}", "s".repeat(5_000)),
                    name: String::new(),
                },
                ChatMessage {
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
                    role: "system".to_owned(),
                    content: "You are Verevon.".to_owned(),
                    name: String::new(),
                },
                ChatMessage {
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
        assert_eq!(min_cacheable_tokens("claude-haiku-4-5"), 2_048);
        assert_eq!(min_cacheable_tokens("claude-sonnet-4-6"), 1_024);
        assert_eq!(min_cacheable_tokens("claude-opus-4-5"), 4_096);
        // Unknown/future deployment names fall back to the Sonnet-class floor.
        assert_eq!(min_cacheable_tokens("some-future-claude"), 1_024);

        // ~6 KiB of prompt ≈ 1500 estimated tokens: over Sonnet's 1024 floor,
        // under Haiku's 2048. The same prompt must cache on one and not the other.
        let mid_sized = |model: &str| InferRequest {
            thinking_budget_tokens: 0,
            model: model.to_owned(),
            max_tokens: 512,
            messages: vec![
                ChatMessage {
                    role: "system".to_owned(),
                    content: "y".repeat(6_000),
                    name: String::new(),
                },
                ChatMessage {
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
