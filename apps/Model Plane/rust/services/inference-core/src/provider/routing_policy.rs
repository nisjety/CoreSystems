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
    /// Ordered ladder of tool-capable models to fall to when the model the
    /// intent layer resolved cannot be served (throttled, or its deployment is
    /// down). Walked in order; entries the resolved model already occupies, and
    /// entries this resource has no deployment for, are skipped.
    ///
    /// It exists because the model-family gate that keeps `claude-*` off the
    /// `OpenAI` surface also means a throttled Claude deployment has nowhere
    /// to go: one 429 on one deployment killed the whole turn. Azure allocates
    /// quota **per deployment**, so a sibling Claude deployment is a real,
    /// usually-healthy alternative rather than a retry of the same bucket.
    ///
    /// Only consulted for a model the intent layer resolved from a `velion-*`
    /// mode — never for a model the caller pinned. An empty ladder disables the
    /// behaviour entirely.
    #[serde(default = "default_tool_fallback_ladder")]
    pub tool_fallback_ladder: Vec<String>,
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
    /// Whether offering tools raises complexity to `Complex` outright, rather
    /// than only adding [`Self::tool_use_score`].
    ///
    /// On by default because scoring alone put tool turns in the WRONG tier: with
    /// the shipped weights, tools contribute 2 against a `complex_threshold` of
    /// 3, so an ordinary tool-bearing question (no code fence, no reasoning
    /// keyword) landed on `Moderate` — a model that receives the tool
    /// definitions and then declines to call them, and answers "I don't have
    /// access" instead. A tool a model won't invoke is worse than no tool: the
    /// user is told the capability is missing when it is right there.
    ///
    /// Operator-tunable because it trades money for reliability: a turn that
    /// would not have needed its tools still pays the top tier for the round
    /// where that is discovered.
    #[serde(default = "default_tool_use_floors_complex")]
    pub tool_use_floors_complex: bool,
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
            tool_fallback_ladder: default_tool_fallback_ladder(),
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

/// Serde default for [`ComplexityWeights::tool_use_floors_complex`] — a policy
/// row stored before the field existed must still floor tool turns, or the
/// mis-tiering it fixes silently returns.
fn default_tool_use_floors_complex() -> bool {
    true
}

/// The shipped tool-capable fallback ladder, in the order it is walked.
///
/// Anthropic first: these are four *separate* Azure Foundry deployments with
/// independent quota, so a 429 on one says nothing about the others, and every
/// one of them calls tools reliably — which is why the intent layer sends tool
/// turns to this family in the first place. Sonnet siblings lead because they
/// are the same capability class as the tier that was throttled; Opus before
/// Haiku because on the second failure of a tool turn, tool-following quality is
/// worth more than the price difference; Haiku closes the family as the cheapest
/// rung that still calls tools.
///
/// The `OpenAI` rungs are last — they cost less but follow tools less reliably,
/// and reaching them means the whole Claude resource is unavailable. `gpt-4o-mini`
/// is final because it is the designated cheap fallback and the deployment most
/// likely to exist on any Azure `OpenAI` resource.
const DEFAULT_TOOL_FALLBACK_LADDER: &[&str] = &[
    "claude-sonnet-4-6",
    "claude-sonnet-4-5",
    "claude-opus-4-8",
    "claude-haiku-4-5",
    "gpt-5-mini",
    "gpt-4o-mini",
];

/// Serde default for [`RoutingPolicy::tool_fallback_ladder`] — a policy row
/// stored before the field existed must still get a ladder, otherwise the
/// dead-end this fixes silently returns for every operator with a stored policy.
fn default_tool_fallback_ladder() -> Vec<String> {
    DEFAULT_TOOL_FALLBACK_LADDER
        .iter()
        .map(|model| (*model).to_owned())
        .collect()
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
            tool_use_floors_complex: true,
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
    fn default_ladder_prefers_anthropic_siblings_then_openai() {
        let ladder = RoutingPolicy::default().tool_fallback_ladder;
        let first_openai = ladder
            .iter()
            .position(|m| !m.starts_with("claude"))
            .expect("the ladder must reach an OpenAI rung");
        let last_claude = ladder
            .iter()
            .rposition(|m| m.starts_with("claude"))
            .expect("the ladder must start in the Claude family");
        assert!(
            last_claude < first_openai,
            "every Claude deployment must be tried before leaving the family: {ladder:?}"
        );
        assert_eq!(ladder.first().map(String::as_str), Some("claude-sonnet-4-6"));
        assert_eq!(ladder.last().map(String::as_str), Some(CHEAP_FALLBACK));
    }

    #[test]
    fn a_policy_stored_before_the_ladder_existed_still_gets_one() {
        // Serde default: rows persisted by an older build carry no
        // `tool_fallback_ladder` key. Deserializing them to an empty ladder
        // would reinstate the dead end for exactly the operators who have a
        // stored policy.
        let json = serde_json::to_string(&serde_json::json!({
            "enabled": true,
            "budget_cap_usd": 50.0,
            "constrained_fraction": 0.8,
            "cheap_fallback": "gpt-4o-mini",
            "complexity": serde_json::to_value(ComplexityWeights::default()).unwrap(),
            "table": serde_json::to_value(RoutingPolicy::default().table).unwrap(),
        }))
        .expect("policy json");
        let parsed: RoutingPolicy = serde_json::from_str(&json).expect("parse legacy policy");
        assert_eq!(parsed.tool_fallback_ladder, default_tool_fallback_ladder());
    }

    #[test]
    fn operators_can_replace_or_disable_the_ladder() {
        let replaced = RoutingPolicy {
            tool_fallback_ladder: vec!["claude-haiku-4-5".to_owned()],
            ..RoutingPolicy::default()
        };
        let round_tripped: RoutingPolicy =
            serde_json::from_str(&serde_json::to_string(&replaced).expect("serialize"))
                .expect("deserialize");
        assert_eq!(round_tripped.tool_fallback_ladder, ["claude-haiku-4-5"]);

        // An explicitly empty ladder must survive the round trip as empty — that
        // is how an operator opts out, and serde(default) must not refill it.
        let disabled_source = RoutingPolicy {
            tool_fallback_ladder: Vec::new(),
            ..RoutingPolicy::default()
        };
        let disabled: RoutingPolicy =
            serde_json::from_str(&serde_json::to_string(&disabled_source).expect("serialize"))
                .expect("deserialize");
        assert!(disabled.tool_fallback_ladder.is_empty());
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
