//! Verevon intent layer — the model-selection step of the API layer.
//!
//! "Verevon" is exposed in the model picker as three auto modes — **Budget**,
//! **Balance**, **Genius** — instead of a single opaque "Verevon Auto". This
//! module turns one of those modes into a concrete model id by combining:
//!
//! 1. **task complexity**, estimated heuristically from the request (message
//!    size, code, reasoning keywords, tool use, conversation depth), and
//! 2. the org's **budget posture**, queried best-effort from cost-core.
//!
//! `gpt-4o-mini` is the designated **cheap fallback** — used when the budget is
//! exhausted. (Azure's `model-router` is a *quality* router that resolves to the
//! newest flagship gpt-5.x, so it is deliberately NOT the cheap fallback.)
//!
//! It runs once at the top of [`crate::provider::fallback::FallbackChain`],
//! before any provider is tried. A pinned model id (e.g. `gpt-4o-mini`,
//! `claude-opus-4-8`) is not a Verevon mode, so it bypasses this layer entirely.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::warn;

use super::routing_policy::RoutingPolicy;
use super::{ChatMessage, ToolDefinition};

/// The three Verevon auto modes the model picker exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerevonMode {
    /// Cost-first: the cheapest capable model.
    Budget,
    /// Balanced cost/quality — the recommended default.
    Balance,
    /// Quality-first: the most capable model the budget allows.
    Genius,
}

impl VerevonMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            VerevonMode::Budget => "budget",
            VerevonMode::Balance => "balance",
            VerevonMode::Genius => "genius",
        }
    }

    /// One tier cheaper — used to tighten routing when the budget is
    /// `Constrained`. `Budget` is already the floor.
    fn downgrade(self) -> VerevonMode {
        match self {
            VerevonMode::Genius => VerevonMode::Balance,
            VerevonMode::Balance | VerevonMode::Budget => VerevonMode::Budget,
        }
    }
}

/// Parse a model id into a Verevon intent mode, or `None` for a pinned model id
/// (which bypasses the intent layer). The literal empty string and `"default"`
/// are intentionally **not** Verevon modes — they keep the legacy per-provider
/// default behaviour for direct, non-UI callers.
#[must_use]
pub fn parse_mode(model: &str) -> Option<VerevonMode> {
    match model.trim().to_ascii_lowercase().as_str() {
        "verevon-budget" => Some(VerevonMode::Budget),
        "verevon-balance" | "verevon" | "verevon-auto" | "auto" => Some(VerevonMode::Balance),
        "verevon-genius" => Some(VerevonMode::Genius),
        _ => None,
    }
}

/// Estimated task complexity, derived heuristically from the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Simple,
    Moderate,
    Complex,
}

impl Complexity {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Complexity::Simple => "simple",
            Complexity::Moderate => "moderate",
            Complexity::Complex => "complex",
        }
    }
}

/// Whether `haystack` contains `needle` as a whole word/phrase — i.e. not
/// embedded inside a larger alphanumeric run. Prevents false positives like
/// `"prove"` firing on `"improve"`/`"approve"`. Both are already lowercased.
fn contains_keyword(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(needle) {
        let start = from + rel;
        let end = start + needle.len();
        let left_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let right_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if left_ok && right_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Heuristic complexity classifier. Pure and deterministic for unit testing.
/// Lengths are counted in Unicode scalar values (not UTF-8 bytes) so that
/// Norwegian text (æ/ø/å) isn't over-classified.
#[must_use]
pub fn classify(
    policy: &RoutingPolicy,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    tool_choice: &str,
) -> Complexity {
    let w = &policy.complexity;
    let mut score: i32 = 0;

    let total_chars: usize = messages.iter().map(|m| m.content.chars().count()).sum();
    if total_chars > w.large_total_chars {
        score += w.large_total_chars_score;
    } else if total_chars > w.medium_total_chars {
        score += w.medium_total_chars_score;
    }

    if let Some(last_user) = messages
        .iter()
        .rev()
        .find(|m| m.role.eq_ignore_ascii_case("user"))
    {
        if last_user.content.chars().count() > w.long_user_turn_chars {
            score += w.long_user_turn_score;
        }
        let lower = last_user.content.to_ascii_lowercase();
        if lower.contains("```") {
            score += w.code_fence_score;
        }
        if w.keywords.iter().any(|kw| contains_keyword(&lower, kw)) {
            score += w.keyword_score;
        }
    }

    // Tool use needs a capable, tool-following model.
    let tools_offered = !tools.is_empty() || tool_choice.eq_ignore_ascii_case("required");
    if tools_offered {
        // Not merely "more expensive" — the cheap tiers are the wrong TOOL for
        // the job. Given real MCP tools they decline to call them and answer "I
        // don't have access", which reads to the user as a missing integration
        // rather than a routing choice. Scoring alone left this one point short
        // of the tier that has a tool-following model, so an ordinary tool
        // question tipped on whether it happened to contain a keyword.
        if w.tool_use_floors_complex {
            return Complexity::Complex;
        }
        score += w.tool_use_score;
    }

    // A long multi-turn conversation carries more context to track.
    if messages.len() > w.deep_conversation_turns {
        score += w.deep_conversation_score;
    }

    if score >= w.complex_threshold {
        Complexity::Complex
    } else if score >= w.moderate_threshold {
        Complexity::Moderate
    } else {
        Complexity::Simple
    }
}

/// Budget posture for an org, derived from cost-core's budget-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetPosture {
    /// Comfortably under the cap (or no cap configured).
    Healthy,
    /// At or above the policy's `constrained_fraction` of the cap — route one
    /// tier cheaper.
    Constrained,
    /// At or over the cap — force the cheap fallback regardless of mode.
    Exhausted,
    /// Could not be determined (no tenant scope, or cost-core unreachable).
    /// Treated like `Healthy` for routing, but surfaced in logs.
    Unknown,
}

/// The default cheap fallback used when the budget is exhausted — a genuinely
/// cheap, reliable, tool-capable model. NOT `model-router`: that router resolves
/// to the newest flagship gpt-5.x (expensive), which would defeat the point of a
/// budget-exhausted fallback. Kept as a named const for the default policy and
/// tests; the live value is `RoutingPolicy::cheap_fallback`.
pub const CHEAP_FALLBACK: &str = "gpt-4o-mini";

/// Resolve a `(mode, complexity, posture)` triple to a concrete model id using
/// the supplied policy. Pure and deterministic for unit testing.
#[must_use]
pub fn choose(
    policy: &RoutingPolicy,
    mode: VerevonMode,
    complexity: Complexity,
    posture: BudgetPosture,
) -> &str {
    // Over budget → always the cheap fallback, whatever the mode or complexity.
    if posture == BudgetPosture::Exhausted {
        return &policy.cheap_fallback;
    }
    // Approaching the cap → behave one tier cheaper.
    let effective = if posture == BudgetPosture::Constrained {
        mode.downgrade()
    } else {
        mode
    };
    policy.cell(effective, complexity)
}

/// The outcome of resolving a Verevon request — the concrete model plus the
/// signals that produced it, for structured logging.
#[derive(Debug, Clone)]
pub struct Decision {
    pub model: String,
    pub mode: VerevonMode,
    pub complexity: Complexity,
    pub posture: BudgetPosture,
}

/// Resolve a Verevon request to a concrete [`Decision`], or `None` when the
/// model id is a pinned model (not a Verevon mode). `budget` is best-effort:
/// when absent, posture is `Unknown` and routing uses the `Healthy` ladder.
/// `caller_bearer` is the caller's own verified token, forwarded to cost-core
/// so the budget check authenticates as the caller; empty skips the check.
#[allow(clippy::too_many_arguments)]
pub async fn resolve(
    policy: &RoutingPolicy,
    model: &str,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    tool_choice: &str,
    org_id: &str,
    user_id: &str,
    caller_bearer: &str,
    budget: Option<&BudgetClient>,
) -> Option<Decision> {
    let mode = parse_mode(model)?;
    let complexity = classify(policy, messages, tools, tool_choice);
    let posture = match budget {
        Some(client) => {
            client
                .posture(
                    org_id,
                    user_id,
                    caller_bearer,
                    policy.budget_cap_usd,
                    policy.constrained_fraction,
                )
                .await
        }
        None => BudgetPosture::Unknown,
    };
    let model = choose(policy, mode, complexity, posture).to_owned();
    Some(Decision {
        model,
        mode,
        complexity,
        posture,
    })
}

/// HTTP client for cost-core's `POST /api/v1/budget/check`. Best-effort: any
/// failure (timeout, missing org, non-2xx, parse error) yields
/// [`BudgetPosture::Unknown`] so a cost-core outage never blocks inference.
#[derive(Clone)]
pub struct BudgetClient {
    http: reqwest::Client,
    endpoint: String,
}

#[derive(Serialize)]
struct BudgetCheckRequest<'a> {
    org_id: &'a str,
    user_id: &'a str,
    max_cost_usd: f64,
    // 0 disables the token cap in cost-core (only the USD cap matters here).
    max_tokens: i64,
}

#[derive(Deserialize)]
struct BudgetCheckResponse {
    allowed: bool,
    #[serde(default)]
    current_cost_usd: f64,
}

impl BudgetClient {
    /// Build a client from cost-core's base URL. Returns `None` for an empty
    /// base URL. The USD cap is no longer frozen here — it is read per-call from
    /// the live [`RoutingPolicy`] and passed into [`Self::posture`].
    #[must_use]
    pub fn new(base_url: &str) -> Option<Self> {
        let base = base_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return None;
        }
        let http = reqwest::Client::builder()
            // Deliberately unchanged at 400ms: measured against the live
            // deployment, cost-core answers this endpoint in 3–70ms on a warm
            // connection, so this is already >5x headroom, and raising it would
            // add that much latency to EVERY inference whenever cost-core is
            // actually down (the gate runs once per infer and degrades silently
            // to `Unknown`).
            .timeout(Duration::from_millis(400))
            // But 400ms is a *total* budget covering DNS + TCP connect, and
            // reqwest's default pool drops an idle connection after 90s. Chat
            // traffic is bursty with long gaps, so the FIRST check of a turn
            // routinely paid a cold setup out of that same 400ms while the
            // checks seconds later reused the socket — which is exactly the
            // shape of the one failure observed in 79 checks over 24h (the
            // turn's main inference logged `Unknown`; its title and follow-up
            // checks moments later both logged `Healthy`). Holding the idle
            // connection much longer keeps the cold path rare without touching
            // the timeout, so this costs no latency in any scenario.
            .pool_idle_timeout(Duration::from_secs(600))
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .ok()?;
        Some(Self {
            http,
            endpoint: format!("{base}/api/v1/budget/check"),
        })
    }

    /// Query the org's budget posture against `cap_usd` (with `fraction` as the
    /// Constrained boundary). `Unknown` when `org_id` is empty (no tenant
    /// scope), when `bearer` is empty (no caller credential to forward — the
    /// check would only 401), or on any transport/parse failure.
    ///
    /// `bearer` is the caller's own verified token (model-gateway's delegated
    /// per-user JWT, `aud=inference-core`), forwarded verbatim: cost-core
    /// wraps `/api/` in JWT auth and `handleBudgetCheck` pins org/user to the
    /// token's claims, so the check must authenticate *as the caller* — a
    /// static service secret could not serve arbitrary orgs.
    pub async fn posture(
        &self,
        org_id: &str,
        user_id: &str,
        bearer: &str,
        cap_usd: f64,
        fraction: f64,
    ) -> BudgetPosture {
        if org_id.trim().is_empty() {
            return BudgetPosture::Unknown;
        }
        let bearer = bearer.trim();
        if bearer.is_empty() {
            warn!("budget check skipped: no caller bearer to forward; posture Unknown");
            return BudgetPosture::Unknown;
        }
        let body = BudgetCheckRequest {
            org_id,
            user_id,
            max_cost_usd: cap_usd,
            max_tokens: 0,
        };
        let resp = match self
            .http
            .post(&self.endpoint)
            .bearer_auth(bearer)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(error) => {
                // `%error` alone prints only reqwest's outermost Display —
                // "error sending request for url (…)" — which names the URL and
                // nothing about the cause. That is why the one observed failure
                // could not be classified after the fact: timeout, refused
                // connection and DNS failure all render identically, and they
                // have completely different fixes. Classify explicitly and walk
                // the source chain so the next occurrence is diagnosable from
                // the log alone.
                warn!(
                    %error,
                    timeout = error.is_timeout(),
                    connect = error.is_connect(),
                    cause = ?std::error::Error::source(&error),
                    "budget check request failed; posture Unknown"
                );
                return BudgetPosture::Unknown;
            }
        };
        if !resp.status().is_success() {
            warn!(status = %resp.status(), "budget check non-2xx; posture Unknown");
            return BudgetPosture::Unknown;
        }
        match resp.json::<BudgetCheckResponse>().await {
            Ok(parsed) => {
                classify_posture(parsed.allowed, parsed.current_cost_usd, cap_usd, fraction)
            }
            Err(error) => {
                warn!(%error, "budget check parse failed; posture Unknown");
                BudgetPosture::Unknown
            }
        }
    }
}

/// Map a budget-check result to a posture. Pure for unit testing.
#[must_use]
pub fn classify_posture(
    allowed: bool,
    current_cost_usd: f64,
    cap_usd: f64,
    fraction: f64,
) -> BudgetPosture {
    if !allowed {
        return BudgetPosture::Exhausted;
    }
    if cap_usd > 0.0 && current_cost_usd >= cap_usd * fraction {
        return BudgetPosture::Constrained;
    }
    BudgetPosture::Healthy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RoutingPolicy {
        RoutingPolicy::default()
    }

    fn user(content: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_owned(),
            content: content.to_owned(),
            name: String::new(),
        }
    }

    #[test]
    fn parse_mode_maps_verevon_ids_only() {
        assert_eq!(parse_mode("verevon-budget"), Some(VerevonMode::Budget));
        assert_eq!(parse_mode("verevon-balance"), Some(VerevonMode::Balance));
        assert_eq!(parse_mode("VEREVON-GENIUS"), Some(VerevonMode::Genius));
        // "Verevon Auto" synonyms route to the balanced mode.
        for m in ["verevon", "verevon-auto", "auto"] {
            assert_eq!(parse_mode(m), Some(VerevonMode::Balance), "{m:?}");
        }
        // Pinned models and the legacy sentinels bypass the intent layer.
        for m in [
            "",
            "default",
            "gpt-4o-mini",
            "claude-opus-4-8",
            "model-router",
        ] {
            assert_eq!(parse_mode(m), None, "{m:?}");
        }
    }

    #[test]
    fn classify_trivial_prompt_is_simple() {
        assert_eq!(
            classify(&policy(), &[user("hi")], &[], "auto"),
            Complexity::Simple
        );
    }

    /// A tool definition shaped like the MCP tools the chat path advertises.
    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_owned(),
            description: String::new(),
            parameters_json: "{}".to_owned(),
        }
    }

    #[test]
    fn an_offered_tool_reaches_the_tool_capable_tier() {
        // The live regression: this exact question, with the 10 Visma MCP tools
        // attached, scored Moderate (tools contribute 2 against a threshold of 3)
        // and was served by a model that then declined to call any of them and
        // told the user it had no Visma access.
        assert_eq!(
            classify(
                &policy(),
                &[user(
                    "Kan du sjekke i Visma hva vi har tomt på lager akkurat nå?"
                )],
                &[tool("mcp__srv__execute_query")],
                "auto"
            ),
            Complexity::Complex
        );
    }

    #[test]
    fn required_tool_choice_reaches_the_tool_capable_tier_without_definitions() {
        assert_eq!(
            classify(&policy(), &[user("hi")], &[], "required"),
            Complexity::Complex
        );
    }

    #[test]
    fn a_toolless_turn_is_unaffected_by_the_floor() {
        // The floor must not quietly promote every turn — a plain question with
        // no tools still tiers on its own merits.
        assert_eq!(
            classify(&policy(), &[user("hi")], &[], "auto"),
            Complexity::Simple
        );
    }

    #[test]
    fn operators_can_disable_the_floor_and_get_scoring_back() {
        let mut p = policy();
        p.complexity.tool_use_floors_complex = false;
        // Back to the old arithmetic: tool_use_score 2 clears moderate (1) but
        // not complex (3).
        assert_eq!(
            classify(
                &p,
                &[user("hi")],
                &[tool("mcp__srv__execute_query")],
                "auto"
            ),
            Complexity::Moderate
        );
    }

    #[test]
    fn a_policy_stored_before_the_floor_existed_still_floors() {
        // Serde default: a row persisted by an older build has no
        // `tool_use_floors_complex` key, and must not silently reinstate the
        // mis-tiering.
        let weights: crate::provider::routing_policy::ComplexityWeights = serde_json::from_str(
            &serde_json::to_string(&serde_json::json!({
                "large_total_chars": 4000, "large_total_chars_score": 2,
                "medium_total_chars": 1200, "medium_total_chars_score": 1,
                "long_user_turn_chars": 800, "long_user_turn_score": 1,
                "code_fence_score": 1, "keyword_score": 1, "tool_use_score": 2,
                "deep_conversation_turns": 12, "deep_conversation_score": 1,
                "moderate_threshold": 1, "complex_threshold": 3, "keywords": []
            }))
            .expect("weights json"),
        )
        .expect("weights without the new key must deserialize");
        assert!(weights.tool_use_floors_complex);
    }

    #[test]
    fn classify_code_or_keywords_lift_to_moderate() {
        assert_eq!(
            classify(
                &policy(),
                &[user("please refactor this function")],
                &[],
                "auto"
            ),
            Complexity::Moderate
        );
        assert_eq!(
            classify(
                &policy(),
                &[user("here is code\n```rust\nfn x(){}\n```")],
                &[],
                "auto"
            ),
            Complexity::Moderate
        );
    }

    #[test]
    fn classify_keyword_match_respects_word_boundaries() {
        // "improve"/"approve" embed "prove" but must NOT trigger the keyword.
        assert_eq!(
            classify(&policy(), &[user("please improve this")], &[], "auto"),
            Complexity::Simple
        );
        assert_eq!(
            classify(
                &policy(),
                &[user("can you approve the change")],
                &[],
                "auto"
            ),
            Complexity::Simple
        );
        // A real whole-word keyword still lifts complexity.
        assert_eq!(
            classify(&policy(), &[user("prove this theorem")], &[], "auto"),
            Complexity::Moderate
        );
    }

    #[test]
    fn classify_counts_chars_not_bytes() {
        // 700 Norwegian 2-byte chars = 1400 bytes but only 700 chars — must stay
        // under the 1200-char "moderate" bar (would trip on byte length).
        let nordic = "å".repeat(700);
        assert_eq!(
            classify(&policy(), &[user(&nordic)], &[], "auto"),
            Complexity::Simple
        );
    }

    #[test]
    fn classify_tool_use_is_complex() {
        let tools = vec![ToolDefinition {
            name: "web_search".to_owned(),
            description: "search".to_owned(),
            parameters_json: "{}".to_owned(),
        }];
        // Tool use (+2) plus a reasoning keyword (+1) clears the Complex bar.
        assert_eq!(
            classify(
                &policy(),
                &[user("analyze the architecture")],
                &tools,
                "auto"
            ),
            Complexity::Complex
        );
    }

    #[test]
    fn classify_long_prompt_is_complex() {
        let long = "x".repeat(4100);
        assert_eq!(
            classify(&policy(), &[user(&long)], &[], "auto"),
            Complexity::Complex
        );
    }

    #[test]
    fn choose_budget_prefers_cheap() {
        let p = policy();
        assert_eq!(
            choose(
                &p,
                VerevonMode::Budget,
                Complexity::Simple,
                BudgetPosture::Healthy
            ),
            "gpt-5-nano"
        );
        assert_eq!(
            choose(
                &p,
                VerevonMode::Budget,
                Complexity::Complex,
                BudgetPosture::Healthy
            ),
            "gpt-5-mini"
        );
    }

    #[test]
    fn choose_balance_scales_with_complexity() {
        let p = policy();
        assert_eq!(
            choose(
                &p,
                VerevonMode::Balance,
                Complexity::Simple,
                BudgetPosture::Healthy
            ),
            "gpt-4o-mini"
        );
        assert_eq!(
            choose(
                &p,
                VerevonMode::Balance,
                Complexity::Moderate,
                BudgetPosture::Healthy
            ),
            "gpt-5-mini"
        );
        assert_eq!(
            choose(
                &p,
                VerevonMode::Balance,
                Complexity::Complex,
                BudgetPosture::Healthy
            ),
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn choose_genius_reaches_for_the_best() {
        let p = policy();
        assert_eq!(
            choose(
                &p,
                VerevonMode::Genius,
                Complexity::Complex,
                BudgetPosture::Healthy
            ),
            "claude-opus-4-8"
        );
        assert_eq!(
            choose(
                &p,
                VerevonMode::Genius,
                Complexity::Moderate,
                BudgetPosture::Healthy
            ),
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn exhausted_budget_forces_cheap_fallback() {
        let p = policy();
        for mode in [
            VerevonMode::Budget,
            VerevonMode::Balance,
            VerevonMode::Genius,
        ] {
            for cx in [
                Complexity::Simple,
                Complexity::Moderate,
                Complexity::Complex,
            ] {
                assert_eq!(
                    choose(&p, mode, cx, BudgetPosture::Exhausted),
                    CHEAP_FALLBACK
                );
            }
        }
    }

    #[test]
    fn constrained_budget_downgrades_one_tier() {
        let p = policy();
        // Genius+Complex normally → opus; constrained → behaves as Balance+Complex → sonnet.
        assert_eq!(
            choose(
                &p,
                VerevonMode::Genius,
                Complexity::Complex,
                BudgetPosture::Constrained
            ),
            "claude-sonnet-4-6"
        );
        // Balance+Complex normally → sonnet; constrained → Budget+Complex → gpt-5-mini.
        assert_eq!(
            choose(
                &p,
                VerevonMode::Balance,
                Complexity::Complex,
                BudgetPosture::Constrained
            ),
            "gpt-5-mini"
        );
    }

    #[test]
    fn unknown_posture_routes_like_healthy() {
        let p = policy();
        assert_eq!(
            choose(
                &p,
                VerevonMode::Genius,
                Complexity::Complex,
                BudgetPosture::Unknown
            ),
            choose(
                &p,
                VerevonMode::Genius,
                Complexity::Complex,
                BudgetPosture::Healthy
            )
        );
    }

    /// A policy with a changed table cell must be honoured by `choose()`.
    #[test]
    fn choose_honours_a_modified_table_cell() {
        let mut p = policy();
        "deepseek-v3-2".clone_into(&mut p.table.genius.complex);
        assert_eq!(
            choose(
                &p,
                VerevonMode::Genius,
                Complexity::Complex,
                BudgetPosture::Healthy
            ),
            "deepseek-v3-2"
        );
        // Default still resolves to opus, proving the change was the policy's.
        assert_eq!(
            choose(
                &policy(),
                VerevonMode::Genius,
                Complexity::Complex,
                BudgetPosture::Healthy
            ),
            "claude-opus-4-8"
        );
    }

    #[test]
    fn classify_posture_thresholds() {
        let frac = policy().constrained_fraction;
        assert_eq!(
            classify_posture(false, 0.0, 50.0, frac),
            BudgetPosture::Exhausted
        );
        assert_eq!(
            classify_posture(true, 10.0, 50.0, frac),
            BudgetPosture::Healthy
        );
        // 80% of the cap is the Constrained boundary.
        assert_eq!(
            classify_posture(true, 40.0, 50.0, frac),
            BudgetPosture::Constrained
        );
        assert_eq!(
            classify_posture(true, 39.99, 50.0, frac),
            BudgetPosture::Healthy
        );
        // No cap configured → never Constrained.
        assert_eq!(
            classify_posture(true, 999.0, 0.0, frac),
            BudgetPosture::Healthy
        );
    }

    #[tokio::test]
    async fn resolve_returns_none_for_pinned_model() {
        let d = resolve(
            &policy(),
            "gpt-4o-mini",
            &[user("hi")],
            &[],
            "auto",
            "org1",
            "u1",
            "bearer-1",
            None,
        )
        .await;
        assert!(d.is_none());
    }

    #[tokio::test]
    async fn resolve_verevon_balance_without_budget_is_unknown_posture() {
        let d = resolve(
            &policy(),
            "verevon-balance",
            &[user("hi")],
            &[],
            "auto",
            "",
            "",
            "",
            None,
        )
        .await
        .expect("verevon mode resolves");
        assert_eq!(d.mode, VerevonMode::Balance);
        assert_eq!(d.complexity, Complexity::Simple);
        assert_eq!(d.posture, BudgetPosture::Unknown);
        assert_eq!(d.model, "gpt-4o-mini");
    }

    #[test]
    fn budget_client_rejects_empty_url() {
        assert!(BudgetClient::new("   ").is_none());
        assert!(BudgetClient::new("http://cost-core:8089").is_some());
    }

    /// The root cause of the inert budget gate: the check posted with no auth
    /// header, cost-core's JWT middleware 401'd it, and posture never left
    /// `Unknown`. The forwarded caller bearer must arrive as `Authorization:
    /// Bearer <token>` for cost-core to pin org/user to its claims.
    #[tokio::test]
    async fn posture_forwards_the_caller_bearer_as_authorization() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .and(header("authorization", "Bearer caller-jwt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "allowed": true,
                "current_cost_usd": 1.0,
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = BudgetClient::new(&server.uri()).expect("client");
        let posture = client.posture("org1", "u1", "caller-jwt", 50.0, 0.8).await;
        assert_eq!(posture, BudgetPosture::Healthy);
        // Mock::expect(1) is verified on MockServer drop: a request without the
        // exact Authorization header would not have matched.
    }

    /// No caller credential → no network call at all (the check could only
    /// 401), and the fail-open contract holds: posture is `Unknown`.
    #[tokio::test]
    async fn posture_with_empty_bearer_short_circuits_to_unknown() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/budget/check"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "allowed": false,
            })))
            .expect(0)
            .mount(&server)
            .await;

        let client = BudgetClient::new(&server.uri()).expect("client");
        for bearer in ["", "   "] {
            assert_eq!(
                client.posture("org1", "u1", bearer, 50.0, 0.8).await,
                BudgetPosture::Unknown,
                "bearer {bearer:?} must skip the check"
            );
        }
        // expect(0) is verified on MockServer drop: any request would fail it.
    }
}
