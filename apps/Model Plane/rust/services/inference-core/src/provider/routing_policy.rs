//! Runtime-tunable configuration for the Velion intent layer.
//!
//! The intent layer ([`super::intent`]) turns a `velion-budget` /
//! `velion-balance` / `velion-genius` mode into a concrete model id by
//! combining heuristic task complexity with the org's budget posture. The
//! exact numbers behind those two steps — the complexity scoring weights, the
//! reasoning-keyword list, and the (mode × complexity) → model table — used to
//! live as compile-time `const`s in `intent.rs`.
//!
//! This struct externalizes them so they can be edited at runtime. The store
//! is owned by session-core (a JSONB singleton); inference-core polls it and
//! falls back to [`RoutingPolicy::default`] whenever the store is empty or
//! unreachable. **[`RoutingPolicy::default`] reproduces the exact values that
//! were inline in `intent.rs`, so default behaviour is byte-identical to the
//! compile-time policy.**

use serde::{Deserialize, Serialize};

use super::intent::{Complexity, VelionMode, CHEAP_FALLBACK};

/// The full runtime routing policy. Serializes as `snake_case` JSON; the
/// canonical schema is this struct (session-core stores it as an opaque string).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RoutingPolicy {
    /// Master switch for the intent layer. When `false`, `velion-*` ids fall
    /// through to the legacy per-provider default resolution.
    pub enabled: bool,
    /// Monthly USD budget cap — the denominator for the budget posture.
    pub budget_cap_usd: f64,
    /// Fraction of the cap at which routing tightens to a cheaper tier.
    pub constrained_fraction: f64,
    /// The model id used when the budget is exhausted (the cheap fallback).
    pub cheap_fallback: String,
    /// Complexity-classifier weights and keyword list.
    pub complexity: ComplexityWeights,
    /// The (mode × complexity) → concrete model id routing table.
    pub table: ModeTable,
}

/// The (mode × complexity) routing table: one [`ComplexityModels`] row per mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ModeTable {
    pub budget: ComplexityModels,
    pub balance: ComplexityModels,
    pub genius: ComplexityModels,
}

/// One mode's row: the model id chosen at each complexity tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ComplexityModels {
    pub simple: String,
    pub moderate: String,
    pub complex: String,
}

/// The complexity-classifier's scoring weights, thresholds and keyword list.
/// Every value here was an inline literal in `intent::classify`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ComplexityWeights {
    /// Total characters above which `+large_total_chars_score` is added.
    pub large_total_chars: usize,
    pub large_total_chars_score: i32,
    /// Total characters above which `+medium_total_chars_score` is added (only
    /// when not already over [`Self::large_total_chars`]).
    pub medium_total_chars: usize,
    pub medium_total_chars_score: i32,
    /// Last-user-turn length above which `+long_user_turn_score` is added.
    pub long_user_turn_chars: usize,
    pub long_user_turn_score: i32,
    /// Added when the last user turn contains a fenced code block (a `` ``` ``
    /// run).
    pub code_fence_score: i32,
    /// Added when the last user turn matches a reasoning keyword.
    pub keyword_score: i32,
    /// Added when tools are present or `tool_choice == "required"`.
    pub tool_use_score: i32,
    /// Conversation turn count above which `+deep_conversation_score` is added.
    pub deep_conversation_turns: usize,
    pub deep_conversation_score: i32,
    /// Score at or above which complexity is `Moderate`.
    pub moderate_threshold: i32,
    /// Score at or above which complexity is `Complex`.
    pub complex_threshold: i32,
    /// Substrings that signal a request needs deeper reasoning. Matched
    /// case-insensitively (whole-word) against the latest user turn.
    pub keywords: Vec<String>,
}

impl Default for RoutingPolicy {
    /// Reproduces the exact compile-time policy that was inline in `intent.rs`.
    fn default() -> Self {
        Self {
            enabled: true,
            budget_cap_usd: 50.0,
            constrained_fraction: 0.8,
            cheap_fallback: CHEAP_FALLBACK.to_owned(),
            complexity: ComplexityWeights::default(),
            // Ladder over the deployed roster, ascending cost/capability:
            //   gpt-5-nano < gpt-4o-mini < gpt-5-mini < claude-sonnet-4-6 < claude-opus-4-8.
            // Each mode owns a distinct cost/quality identity; complexity climbs
            // the row. A `Constrained` budget shifts one mode cheaper (Genius→
            // Balance→Budget), and an `Exhausted` budget forces `cheap_fallback`.
            table: ModeTable {
                // Budget — cheapest capable model at every tier.
                budget: ComplexityModels {
                    simple: "gpt-5-nano".to_owned(),
                    moderate: "gpt-4o-mini".to_owned(),
                    complex: "gpt-5-mini".to_owned(),
                },
                // Balance — cost/quality sweet spot; reaches a strong reasoner
                // (Claude Sonnet) only for genuinely complex work.
                balance: ComplexityModels {
                    simple: "gpt-4o-mini".to_owned(),
                    moderate: "gpt-5-mini".to_owned(),
                    complex: "claude-sonnet-4-6".to_owned(),
                },
                // Genius — best/smartest; tops out at Claude Opus for complex
                // work, but stays efficient on trivial turns.
                genius: ComplexityModels {
                    simple: "gpt-5-mini".to_owned(),
                    moderate: "claude-sonnet-4-6".to_owned(),
                    complex: "claude-opus-4-8".to_owned(),
                },
            },
        }
    }
}

impl Default for ComplexityWeights {
    /// The inline literals from `intent::classify`:
    /// chars>4000 → +2, >1200 → +1; last-user>800 → +1; code-fence → +1;
    /// keyword → +1; tool/required → +2; >12 turns → +1; moderate ≥ 1; complex ≥ 3.
    fn default() -> Self {
        Self {
            large_total_chars: 4000,
            large_total_chars_score: 2,
            medium_total_chars: 1200,
            medium_total_chars_score: 1,
            long_user_turn_chars: 800,
            long_user_turn_score: 1,
            code_fence_score: 1,
            keyword_score: 1,
            tool_use_score: 2,
            deep_conversation_turns: 12,
            deep_conversation_score: 1,
            moderate_threshold: 1,
            complex_threshold: 3,
            keywords: COMPLEX_KEYWORDS.iter().map(|kw| (*kw).to_owned()).collect(),
        }
    }
}

/// The reasoning-keyword list, verbatim from `intent.rs`'s `COMPLEX_KEYWORDS`.
const COMPLEX_KEYWORDS: &[&str] = &[
    "step by step",
    "step-by-step",
    "prove",
    "derive",
    "analyze",
    "analyse",
    "architecture",
    "refactor",
    "debug",
    "optimize",
    "optimise",
    "explain why",
    "compare",
    "trade-off",
    "tradeoff",
    "algorithm",
    "root cause",
    "in depth",
    "in-depth",
    "comprehensive",
    "reason through",
];

impl RoutingPolicy {
    /// The model id for a `(mode, complexity)` pair, before any budget posture
    /// adjustment. Mirrors the old `choose()` table body.
    #[must_use]
    pub fn cell(&self, mode: VelionMode, complexity: Complexity) -> &str {
        let row = match mode {
            VelionMode::Budget => &self.table.budget,
            VelionMode::Balance => &self.table.balance,
            VelionMode::Genius => &self.table.genius,
        };
        match complexity {
            Complexity::Simple => &row.simple,
            Complexity::Moderate => &row.moderate,
            Complexity::Complex => &row.complex,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::intent::{choose, BudgetPosture};

    #[test]
    fn serde_round_trip_preserves_policy() {
        let policy = RoutingPolicy::default();
        let json = serde_json::to_string(&policy).expect("serialize");
        let parsed: RoutingPolicy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(policy, parsed);
    }

    #[test]
    fn serde_uses_snake_case_table_shape() {
        // The table must (de)serialize as {"budget":{"simple":...},...}.
        let policy = RoutingPolicy::default();
        let value = serde_json::to_value(&policy).expect("to_value");
        assert_eq!(value["table"]["budget"]["simple"], "gpt-5-nano");
        assert_eq!(value["table"]["balance"]["complex"], "claude-sonnet-4-6");
        assert_eq!(value["table"]["genius"]["complex"], "claude-opus-4-8");
        assert_eq!(value["budget_cap_usd"], 50.0);
        assert_eq!(value["cheap_fallback"], "gpt-4o-mini");
        assert_eq!(value["complexity"]["large_total_chars"], 4000);
    }

    #[test]
    fn parses_external_table_json() {
        let json = r#"{
            "enabled": true,
            "budget_cap_usd": 50.0,
            "constrained_fraction": 0.8,
            "cheap_fallback": "model-router",
            "complexity": {
                "large_total_chars": 4000,
                "large_total_chars_score": 2,
                "medium_total_chars": 1200,
                "medium_total_chars_score": 1,
                "long_user_turn_chars": 800,
                "long_user_turn_score": 1,
                "code_fence_score": 1,
                "keyword_score": 1,
                "tool_use_score": 2,
                "deep_conversation_turns": 12,
                "deep_conversation_score": 1,
                "moderate_threshold": 1,
                "complex_threshold": 3,
                "keywords": ["prove", "derive"]
            },
            "table": {
                "budget": {"simple": "gpt-4o-mini", "moderate": "model-router", "complex": "model-router"},
                "balance": {"simple": "gpt-4o-mini", "moderate": "model-router", "complex": "claude-sonnet-4-6"},
                "genius": {"simple": "gpt-4o-mini", "moderate": "claude-sonnet-4-6", "complex": "claude-opus-4-8"}
            }
        }"#;
        let parsed: RoutingPolicy = serde_json::from_str(json).expect("parse external json");
        assert_eq!(
            parsed.cell(VelionMode::Genius, Complexity::Complex),
            "claude-opus-4-8"
        );
        assert_eq!(parsed.complexity.keywords, vec!["prove", "derive"]);
    }

    /// The Default policy's routing must match the old compile-time `choose()`
    /// table for every (mode, complexity) under the Healthy ladder.
    #[test]
    fn default_matches_old_choose_for_all_cells() {
        let policy = RoutingPolicy::default();
        for mode in [VelionMode::Budget, VelionMode::Balance, VelionMode::Genius] {
            for cx in [
                Complexity::Simple,
                Complexity::Moderate,
                Complexity::Complex,
            ] {
                assert_eq!(
                    policy.cell(mode, cx),
                    choose(&policy, mode, cx, BudgetPosture::Healthy),
                    "cell mismatch for {mode:?}/{cx:?}"
                );
            }
        }
    }

    /// Spot-check every Default cell against the designed ladder — Budget
    /// cheapest, Balance mid, Genius smartest; complexity climbs each row.
    #[test]
    fn default_cells_match_documented_values() {
        let policy = RoutingPolicy::default();
        // Budget — cheapest tier.
        assert_eq!(
            policy.cell(VelionMode::Budget, Complexity::Simple),
            "gpt-5-nano"
        );
        assert_eq!(
            policy.cell(VelionMode::Budget, Complexity::Moderate),
            "gpt-4o-mini"
        );
        assert_eq!(
            policy.cell(VelionMode::Budget, Complexity::Complex),
            "gpt-5-mini"
        );
        // Balance — rises to a strong reasoner only when complex.
        assert_eq!(
            policy.cell(VelionMode::Balance, Complexity::Simple),
            "gpt-4o-mini"
        );
        assert_eq!(
            policy.cell(VelionMode::Balance, Complexity::Moderate),
            "gpt-5-mini"
        );
        assert_eq!(
            policy.cell(VelionMode::Balance, Complexity::Complex),
            "claude-sonnet-4-6"
        );
        // Genius — tops out at Opus, efficient on trivial turns.
        assert_eq!(
            policy.cell(VelionMode::Genius, Complexity::Simple),
            "gpt-5-mini"
        );
        assert_eq!(
            policy.cell(VelionMode::Genius, Complexity::Moderate),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            policy.cell(VelionMode::Genius, Complexity::Complex),
            "claude-opus-4-8"
        );
    }
}
