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
