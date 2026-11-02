//! Extended-thinking effort profiles (Quick / Standard / Deep).
//!
//! The dial a caller turns; the token budget is derived here rather than sent by
//! the client, so a client cannot ask for an arbitrary budget — the profiles are
//! the contract, exactly as `crate::verbosity` makes response style a profile
//! rather than a free-text directive.
//!
//! # Why the default is no thinking
//!
//! `standard` (and unset, and anything unrecognised) requests **no** thinking, so
//! the default turn is byte-identical to before this existed. Extended thinking
//! costs latency and tokens on every turn it is enabled for, and most turns do
//! not need it; making it opt-in keeps the ordinary chat path unchanged and
//! makes "deep" mean something.

use mp_contracts::model_plane::v1::ReasoningEffort;

/// Budget for `deep`. Comfortably above Anthropic's 1024-token floor and enough
/// for genuinely multi-step reasoning without dominating the response ceiling.
const DEEP_BUDGET_TOKENS: i32 = 4096;

/// Budget for `quick` — a short scratchpad, still above the provider floor.
const QUICK_BUDGET_TOKENS: i32 = 1024;

/// Headroom the answer must retain after the thinking budget is carved out.
///
/// The budget is spent *from* `max_tokens`, so a budget that consumes most of
/// the ceiling produces a well-reasoned truncated answer — the worst of both.
/// A profile that cannot fit inside the ceiling with this much left over is
/// dropped to the next one down, rather than silently starving the reply.
const MIN_ANSWER_HEADROOM_TOKENS: i32 = 1024;

/// Thinking budget for an effort profile, or 0 for "no thinking".
///
/// `max_tokens` is the response ceiling for the turn; the returned budget is
/// always small enough to leave [`MIN_ANSWER_HEADROOM_TOKENS`] for the answer.
#[must_use]
pub fn budget_tokens(profile: &str, max_tokens: i32) -> i32 {
    let requested = match profile.trim().to_ascii_lowercase().as_str() {
        "deep" | "extended" | "thorough" => DEEP_BUDGET_TOKENS,
        "quick" | "brief" => QUICK_BUDGET_TOKENS,
        // "standard", "", and anything unrecognised: no thinking.
        _ => return 0,
    };
    // Step down rather than truncate the answer.
    if max_tokens - requested >= MIN_ANSWER_HEADROOM_TOKENS {
        return requested;
    }
    if requested > QUICK_BUDGET_TOKENS
        && max_tokens - QUICK_BUDGET_TOKENS >= MIN_ANSWER_HEADROOM_TOKENS
    {
        return QUICK_BUDGET_TOKENS;
    }
    0
}

/// Whether a profile requests any thinking at all.
#[must_use]
pub fn is_active(profile: &str, max_tokens: i32) -> bool {
    budget_tokens(profile, max_tokens) > 0
}

// # The effort ladder
//
// With `MODEL_GATEWAY_EFFORT_LADDER` on, the gateway no longer turns a profile
// into a token budget. It sends the canonical level and a budget of 0, and
// inference-core renders the level for whichever model serves each attempt
// (so a fallback re-resolves rather than inheriting a number sized for another
// model). This module is the only place the profile is interpreted: the normal
// turn, the ZDR turn and the source-validation check all call [`plan`] or
// [`source_review_plan`], so the three paths cannot drift apart again.

/// The switch. Off keeps the legacy budget and is byte-identical to before.
pub const LADDER_ENV: &str = "MODEL_GATEWAY_EFFORT_LADDER";

/// The most completion tokens a request may ask for once a reasoning allowance
/// is added to its ceiling. Mirrors inference-core's `effort::TOTAL_TOKEN_CAP`;
/// the contract test `allowance_table_matches_inference_core` pins both.
const TOTAL_TOKEN_CAP: i32 = 32_768;

/// Whether an explicit value turns the ladder on. Pure so the policy is
/// testable without touching the process environment; a stray `1` or `on` must
/// not start spending reasoning allowance.
#[must_use]
pub fn parse_ladder(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// Whether the ladder is on for this process.
#[must_use]
pub fn ladder_enabled() -> bool {
    parse_ladder(std::env::var(LADDER_ENV).ok().as_deref())
}

/// What one request asks of the provider layer for reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Legacy `InferRequest.thinking_budget_tokens`. Always 0 when a level is
    /// set: the level wins and a budget beside it would be ignored.
    pub budget_tokens: i32,
    /// `InferRequest.reasoning_effort`.
    pub effort: ReasoningEffort,
    /// Completion tokens the level may spend on reasoning on top of the answer
    /// ceiling, for the truncation check. 0 without a level.
    pub allowance_tokens: i32,
}

impl Plan {
    /// No thinking: the default turn.
    pub const NONE: Plan = Plan {
        budget_tokens: 0,
        effort: ReasoningEffort::Unspecified,
        allowance_tokens: 0,
    };

    /// `effort` as the wire number.
    #[must_use]
    pub fn effort_wire(self) -> i32 {
        self.effort as i32
    }
}

/// The level a profile names: `quick`/`brief` are Low, `deep`/`extended`/
/// `thorough` are High, the canonical names pass through, and `standard`, unset
/// and anything unrecognised are Unspecified (no reasoning request).
///
/// An unrecognised value is counted and logged rather than refused: a stale
/// client must not lose its turn over a profile name, but a typo must not be
/// invisible either.
#[must_use]
pub fn effort_level(profile: &str) -> ReasoningEffort {
    let name = profile.trim().to_ascii_lowercase();
    match name.as_str() {
        "" | "standard" => ReasoningEffort::Unspecified,
        "quick" | "brief" | "low" => ReasoningEffort::Low,
        "deep" | "extended" | "thorough" | "high" => ReasoningEffort::High,
        "minimal" => ReasoningEffort::Minimal,
        "medium" => ReasoningEffort::Medium,
        "xhigh" => ReasoningEffort::Xhigh,
        "max" => ReasoningEffort::Max,
        _ => {
            metrics::counter!("gateway_effort_unknown_total").increment(1);
            tracing::warn!(
                profile = %name.chars().take(32).collect::<String>(),
                "unknown effort profile treated as standard"
            );
            ReasoningEffort::Unspecified
        }
    }
}

/// The effort level a fresh Do or Plan run carries on its rounds, or 0 for none.
///
/// On, this is the profile's own level from [`effort_level`], the same derivation
/// [`plan`] gives the chat turn, so a run and a turn cannot disagree about what `deep`
/// means. Off, the profile is not consulted at all: the request stays what it was, and
/// the unknown-profile counter moves only on the ladder path, as it does for the turn.
#[must_use]
pub fn run_level(profile: &str, ladder_on: bool) -> i32 {
    if ladder_on {
        effort_level(profile) as i32
    } else {
        0
    }
}

/// Completion tokens a level may spend on reasoning. Mirrors inference-core's
/// `effort::allowance`, which is what its adapters add to the ceiling.
#[must_use]
pub fn reasoning_allowance(level: ReasoningEffort) -> i32 {
    match level {
        ReasoningEffort::Unspecified => 0,
        ReasoningEffort::Minimal => 512,
        ReasoningEffort::Low => 1_024,
        ReasoningEffort::Medium => 2_048,
        ReasoningEffort::High => 4_096,
        ReasoningEffort::Xhigh => 8_192,
        ReasoningEffort::Max => 16_384,
    }
}

/// [`reasoning_allowance`] for a wire level, for a caller that holds only the
/// request. A value this build does not know allows nothing: inference-core
/// refuses such a request, so there is no answer to score.
#[must_use]
pub fn allowance_for_wire(value: i32) -> i32 {
    ReasoningEffort::try_from(value).map_or(0, reasoning_allowance)
}

/// The ceiling the provider layer will actually allow once the allowance is
/// added: the caller's own ceiling plus the allowance, bounded by the total
/// cap, and never below the caller's own ceiling.
#[must_use]
pub fn raised_ceiling(ceiling: i32, allowance_tokens: i32) -> i32 {
    ceiling.max(ceiling.saturating_add(allowance_tokens).min(TOTAL_TOKEN_CAP))
}

/// The reasoning request for a profile under a response ceiling.
///
/// Off, this is exactly the legacy budget (including its step-down under a
/// tight ceiling). On, the ceiling is not consulted: the level is sent with no
/// budget and the provider layer raises the ceiling by the level's allowance.
#[must_use]
pub fn plan(profile: &str, ceiling: i32, ladder_on: bool) -> Plan {
    if !ladder_on {
        return Plan {
            budget_tokens: budget_tokens(profile, ceiling),
            ..Plan::NONE
        };
    }
    let effort = effort_level(profile);
    Plan {
        budget_tokens: 0,
        effort,
        allowance_tokens: reasoning_allowance(effort),
    }
}

/// The reasoning request of a source-validation check. A semantic acceptance
/// decision needs reasoning whatever profile the author used, so it is fixed:
/// the legacy 4096-token budget off, High on.
#[must_use]
pub fn source_review_plan(ladder_on: bool) -> Plan {
    if ladder_on {
        Plan {
            budget_tokens: 0,
            effort: ReasoningEffort::High,
            allowance_tokens: reasoning_allowance(ReasoningEffort::High),
        }
    } else {
        Plan {
            budget_tokens: DEEP_BUDGET_TOKENS,
            ..Plan::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default path must be unchanged by this module's existence.
    #[test]
    fn standard_and_unknown_request_no_thinking() {
        for profile in ["standard", "", "   ", "wat", "NORMAL"] {
            assert_eq!(
                budget_tokens(profile, 8192),
                0,
                "profile {profile:?} must not enable thinking"
            );
            assert!(!is_active(profile, 8192));
        }
    }

    #[test]
    fn profiles_map_to_their_budgets() {
        for profile in ["deep", "Deep", " EXTENDED ", "thorough"] {
            assert_eq!(budget_tokens(profile, 8192), DEEP_BUDGET_TOKENS);
        }
        for profile in ["quick", "Brief"] {
            assert_eq!(budget_tokens(profile, 8192), QUICK_BUDGET_TOKENS);
        }
    }

    /// A budget is spent from the response ceiling, so a deep budget under a
    /// tight ceiling must step down instead of starving the answer.
    #[test]
    fn a_tight_ceiling_steps_down_rather_than_truncating_the_answer() {
        // 4096 deep + 1024 headroom needs 5120; 4096 total cannot fit it.
        assert_eq!(budget_tokens("deep", 4096), QUICK_BUDGET_TOKENS);
        // Nothing fits with headroom under 2048.
        assert_eq!(budget_tokens("deep", 2047), 0);
        assert_eq!(budget_tokens("quick", 2047), 0);
        // Exactly enough for quick + headroom.
        assert_eq!(budget_tokens("quick", 2048), QUICK_BUDGET_TOKENS);
    }

    #[test]
    fn only_the_literal_true_turns_the_ladder_on() {
        assert!(parse_ladder(Some("true")));
        assert!(parse_ladder(Some(" true ")));
        for raw in [None, Some(""), Some("1"), Some("on"), Some("TRUE"), Some("yes")] {
            assert!(!parse_ladder(raw), "{raw:?} must not enable the ladder");
        }
    }

    #[test]
    fn profile_maps_to_level() {
        for (profile, level) in [
            ("quick", ReasoningEffort::Low),
            ("Brief", ReasoningEffort::Low),
            ("deep", ReasoningEffort::High),
            (" EXTENDED ", ReasoningEffort::High),
            ("thorough", ReasoningEffort::High),
            ("standard", ReasoningEffort::Unspecified),
            ("", ReasoningEffort::Unspecified),
            ("   ", ReasoningEffort::Unspecified),
        ] {
            assert_eq!(effort_level(profile), level, "{profile:?}");
        }
    }

    #[test]
    fn canonical_names_pass_through() {
        for (name, level) in [
            ("minimal", ReasoningEffort::Minimal),
            ("low", ReasoningEffort::Low),
            ("medium", ReasoningEffort::Medium),
            ("high", ReasoningEffort::High),
            ("xhigh", ReasoningEffort::Xhigh),
            ("MAX", ReasoningEffort::Max),
        ] {
            assert_eq!(effort_level(name), level, "{name}");
        }
    }

    #[test]
    fn unknown_effort_is_standard() {
        for profile in ["wat", "NORMAL", "ultra", "deep!"] {
            assert_eq!(effort_level(profile), ReasoningEffort::Unspecified, "{profile}");
        }
    }

    /// With the ladder on, a run carries the level its profile names, the number the chat
    /// turn sends for the same profile: `deep` is High (4) on the wire.
    #[test]
    fn deep_is_high_with_the_ladder_on() {
        assert_eq!(run_level("deep", true), ReasoningEffort::High as i32);
        assert_eq!(run_level("deep", true), 4);
        assert_eq!(run_level(" EXTENDED ", true), 4);
        assert_eq!(
            run_level("deep", true),
            plan("deep", 4096, true).effort_wire()
        );
    }

    /// With the ladder off no profile reaches the wire as a level, whatever it names.
    #[test]
    fn anything_is_unspecified_with_the_ladder_off() {
        for profile in ["deep", "quick", "max", "standard", "wat", ""] {
            assert_eq!(run_level(profile, false), 0, "{profile:?}");
        }
    }

    /// `standard`, unset and blank name no level even with the ladder on, so a default turn
    /// keeps its request free of reasoning.
    #[test]
    fn standard_and_unset_are_unspecified() {
        for profile in ["standard", "", "   "] {
            assert_eq!(run_level(profile, true), 0, "{profile:?}");
        }
    }

    /// Off, the plan is the legacy budget and nothing else: the default deploy
    /// must be byte-identical to before the ladder existed.
    #[test]
    fn flag_off_sends_legacy_budget_only() {
        for profile in ["quick", "deep", "standard", "high", "wat"] {
            for ceiling in [0, 2047, 4096, 8192] {
                let planned = plan(profile, ceiling, false);
                assert_eq!(planned.budget_tokens, budget_tokens(profile, ceiling));
                assert_eq!(planned.effort, ReasoningEffort::Unspecified);
                assert_eq!(planned.allowance_tokens, 0);
            }
        }
    }

    #[test]
    fn flag_on_sends_level_and_zero_budget() {
        let deep = plan("deep", 4096, true);
        assert_eq!(deep.budget_tokens, 0);
        assert_eq!(deep.effort, ReasoningEffort::High);
        assert_eq!(deep.allowance_tokens, 4_096);
        // `deep` no longer steps down to `quick` under the default ceiling: the
        // provider layer raises the ceiling instead (D1 in the plan).
        assert_eq!(plan("quick", 4096, true).effort, ReasoningEffort::Low);
        assert_eq!(plan("standard", 4096, true), Plan::NONE);
        assert_eq!(plan("", 4096, true), Plan::NONE);
    }

    #[test]
    fn allowance_is_monotonic_and_zero_for_no_level() {
        assert_eq!(reasoning_allowance(ReasoningEffort::Unspecified), 0);
        let ladder = [
            ReasoningEffort::Minimal,
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::Xhigh,
            ReasoningEffort::Max,
        ];
        for pair in ladder.windows(2) {
            assert!(reasoning_allowance(pair[0]) < reasoning_allowance(pair[1]));
        }
    }

    #[test]
    fn raised_ceiling_adds_the_allowance_within_the_cap() {
        assert_eq!(raised_ceiling(4096, 0), 4096);
        assert_eq!(raised_ceiling(4096, 4096), 8192);
        assert_eq!(raised_ceiling(30_000, 16_384), TOTAL_TOKEN_CAP);
        // A ceiling the caller already set above the cap is never lowered.
        assert_eq!(raised_ceiling(40_000, 4096), 40_000);
    }

    #[test]
    fn source_review_is_high_effort_on_and_the_legacy_budget_off() {
        assert_eq!(
            source_review_plan(false),
            Plan { budget_tokens: 4_096, ..Plan::NONE }
        );
        let on = source_review_plan(true);
        assert_eq!(on.budget_tokens, 0);
        assert_eq!(on.effort, ReasoningEffort::High);
    }

    /// Whatever the profile, the answer keeps its headroom. This is the
    /// invariant; the specific budgets are tuning.
    #[test]
    fn the_answer_always_keeps_its_headroom() {
        for profile in ["deep", "quick", "standard"] {
            for ceiling in [0, 1, 512, 1024, 2048, 4096, 8192, 32_768] {
                let budget = budget_tokens(profile, ceiling);
                assert!(
                    budget == 0 || ceiling - budget >= MIN_ANSWER_HEADROOM_TOKENS,
                    "profile {profile:?} at ceiling {ceiling} left only \
                     {} for the answer",
                    ceiling - budget
                );
            }
        }
    }
}
