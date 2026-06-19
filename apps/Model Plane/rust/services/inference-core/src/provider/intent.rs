//! Velion intent layer — the model-selection step of the API layer.
//!
//! "Velion" is exposed in the model picker as three auto modes — **Budget**,
//! **Balance**, **Genius** — instead of a single opaque "Velion Auto". This
//! module turns one of those modes into a concrete model id by combining:
//!
//! 1. **task complexity**, estimated heuristically from the request (message
//!    size, code, reasoning keywords, tool use, conversation depth), and
//! 2. the org's **budget posture**, queried best-effort from cost-core.
//!
//! `model-router` (Azure's server-side cost-optimizing auto-router) is the
//! designated **cheap fallback** — used when the budget is exhausted.
//!
//! It runs once at the top of [`crate::provider::fallback::FallbackChain`],
//! before any provider is tried. A pinned model id (e.g. `gpt-4o-mini`,
//! `claude-opus-4-8`) is not a Velion mode, so it bypasses this layer entirely.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::warn;

use super::routing_policy::RoutingPolicy;
use super::{ChatMessage, ToolDefinition};

/// The three Velion auto modes the model picker exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VelionMode {
    /// Cost-first: the cheapest capable model.
    Budget,
    /// Balanced cost/quality — the recommended default.
    Balance,
    /// Quality-first: the most capable model the budget allows.
    Genius,
}

impl VelionMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            VelionMode::Budget => "budget",
            VelionMode::Balance => "balance",
            VelionMode::Genius => "genius",
        }
    }

    /// One tier cheaper — used to tighten routing when the budget is
    /// `Constrained`. `Budget` is already the floor.
    fn downgrade(self) -> VelionMode {
        match self {
            VelionMode::Genius => VelionMode::Balance,
            VelionMode::Balance | VelionMode::Budget => VelionMode::Budget,
        }
    }
}

/// Parse a model id into a Velion intent mode, or `None` for a pinned model id
/// (which bypasses the intent layer). The literal empty string and `"default"`
/// are intentionally **not** Velion modes — they keep the legacy per-provider
/// default behaviour for direct, non-UI callers.
#[must_use]
pub fn parse_mode(model: &str) -> Option<VelionMode> {
    match model.trim().to_ascii_lowercase().as_str() {
        "velion-budget" => Some(VelionMode::Budget),
        "velion-balance" | "velion" | "velion-auto" | "auto" => Some(VelionMode::Balance),
        "velion-genius" => Some(VelionMode::Genius),
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
    if !tools.is_empty() || tool_choice.eq_ignore_ascii_case("required") {
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

/// Azure's server-side cost-optimizing auto-router — the default cheap
/// fallback. Kept as a named const for the default policy and tests; the live
/// value is `RoutingPolicy::cheap_fallback`.
pub const CHEAP_FALLBACK: &str = "model-router";

/// Resolve a `(mode, complexity, posture)` triple to a concrete model id using
/// the supplied policy. Pure and deterministic for unit testing.
#[must_use]
pub fn choose(
    policy: &RoutingPolicy,
    mode: VelionMode,
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

/// The outcome of resolving a Velion request — the concrete model plus the
/// signals that produced it, for structured logging.
#[derive(Debug, Clone)]
pub struct Decision {
    pub model: String,
    pub mode: VelionMode,
    pub complexity: Complexity,
    pub posture: BudgetPosture,
}

/// Resolve a Velion request to a concrete [`Decision`], or `None` when the
/// model id is a pinned model (not a Velion mode). `budget` is best-effort:
/// when absent, posture is `Unknown` and routing uses the `Healthy` ladder.
#[allow(clippy::too_many_arguments)]
pub async fn resolve(
    policy: &RoutingPolicy,
    model: &str,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    tool_choice: &str,
    org_id: &str,
    user_id: &str,
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
            .timeout(Duration::from_millis(400))
            .build()
            .ok()?;
        Some(Self {
            http,
            endpoint: format!("{base}/api/v1/budget/check"),
        })
    }

    /// Query the org's budget posture against `cap_usd` (with `fraction` as the
    /// Constrained boundary). `Unknown` when `org_id` is empty (no tenant scope)
    /// or on any transport/parse failure.
    pub async fn posture(
        &self,
        org_id: &str,
        user_id: &str,
        cap_usd: f64,
        fraction: f64,
    ) -> BudgetPosture {
        if org_id.trim().is_empty() {
            return BudgetPosture::Unknown;
        }
        let body = BudgetCheckRequest {
            org_id,
            user_id,
            max_cost_usd: cap_usd,
            max_tokens: 0,
        };
        let resp = match self.http.post(&self.endpoint).json(&body).send().await {
            Ok(r) => r,
            Err(error) => {
                warn!(%error, "budget check request failed; posture Unknown");
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
    fn parse_mode_maps_velion_ids_only() {
        assert_eq!(parse_mode("velion-budget"), Some(VelionMode::Budget));
        assert_eq!(parse_mode("velion-balance"), Some(VelionMode::Balance));
        assert_eq!(parse_mode("VELION-GENIUS"), Some(VelionMode::Genius));
        // "Velion Auto" synonyms route to the balanced mode.
        for m in ["velion", "velion-auto", "auto"] {
            assert_eq!(parse_mode(m), Some(VelionMode::Balance), "{m:?}");
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
                VelionMode::Budget,
                Complexity::Simple,
                BudgetPosture::Healthy
            ),
            "gpt-4o-mini"
        );
        assert_eq!(
            choose(
                &p,
                VelionMode::Budget,
                Complexity::Complex,
                BudgetPosture::Healthy
            ),
            "model-router"
        );
    }

    #[test]
    fn choose_balance_scales_with_complexity() {
        let p = policy();
        assert_eq!(
            choose(
                &p,
                VelionMode::Balance,
                Complexity::Simple,
                BudgetPosture::Healthy
            ),
            "gpt-4o-mini"
        );
        assert_eq!(
            choose(
                &p,
                VelionMode::Balance,
                Complexity::Moderate,
                BudgetPosture::Healthy
            ),
            "model-router"
        );
        assert_eq!(
            choose(
                &p,
                VelionMode::Balance,
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
                VelionMode::Genius,
                Complexity::Complex,
                BudgetPosture::Healthy
            ),
            "claude-opus-4-8"
        );
        assert_eq!(
            choose(
                &p,
                VelionMode::Genius,
                Complexity::Moderate,
                BudgetPosture::Healthy
            ),
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn exhausted_budget_forces_cheap_fallback() {
        let p = policy();
        for mode in [VelionMode::Budget, VelionMode::Balance, VelionMode::Genius] {
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
                VelionMode::Genius,
                Complexity::Complex,
                BudgetPosture::Constrained
            ),
            "claude-sonnet-4-6"
        );
        // Balance+Complex normally → sonnet; constrained → Budget+Complex → router.
        assert_eq!(
            choose(
                &p,
                VelionMode::Balance,
                Complexity::Complex,
                BudgetPosture::Constrained
            ),
            "model-router"
        );
    }

    #[test]
    fn unknown_posture_routes_like_healthy() {
        let p = policy();
        assert_eq!(
            choose(
                &p,
                VelionMode::Genius,
                Complexity::Complex,
                BudgetPosture::Unknown
            ),
            choose(
                &p,
                VelionMode::Genius,
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
                VelionMode::Genius,
                Complexity::Complex,
                BudgetPosture::Healthy
            ),
            "deepseek-v3-2"
        );
        // Default still resolves to opus, proving the change was the policy's.
        assert_eq!(
            choose(
                &policy(),
                VelionMode::Genius,
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
            None,
        )
        .await;
        assert!(d.is_none());
    }

    #[tokio::test]
    async fn resolve_velion_balance_without_budget_is_unknown_posture() {
        let d = resolve(
            &policy(),
            "velion-balance",
            &[user("hi")],
            &[],
            "auto",
            "",
            "",
            None,
        )
        .await
        .expect("velion mode resolves");
        assert_eq!(d.mode, VelionMode::Balance);
        assert_eq!(d.complexity, Complexity::Simple);
        assert_eq!(d.posture, BudgetPosture::Unknown);
        assert_eq!(d.model, "gpt-4o-mini");
    }

    #[test]
    fn budget_client_rejects_empty_url() {
        assert!(BudgetClient::new("   ").is_none());
        assert!(BudgetClient::new("http://cost-core:8089").is_some());
    }
}
