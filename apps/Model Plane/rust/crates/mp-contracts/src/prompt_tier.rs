//! The tiers of a chat prompt, from the text that never changes to the turn
//! being answered (build plan WS02-S5).
//!
//! A provider caches the longest prefix a prompt repeats, so a prompt is only
//! as cacheable as its order: one block that changes every turn, placed ahead
//! of the conversation, invalidates everything after it. Each block belongs to
//! exactly one tier, and a prompt is well ordered when its tiers never
//! decrease from the first message to the last.
//!
//! * [`PromptTier::Stable`]: the standing platform text. Changes on a deploy.
//! * [`PromptTier::Context`]: text authored for the org, Space, persona and
//!   user. Changes when someone edits it.
//! * [`PromptTier::Conversation`]: the thread so far, pins and summary
//!   included. Grows by appending.
//! * [`PromptTier::Volatile`]: blocks rebuilt for each turn (the date, grounding,
//!   memory recall, skills). Different on most turns.
//! * [`PromptTier::Current`]: the user turn being answered.
//!
//! The variants are declared in order and derive `Ord`, so the order is the
//! declaration. `execution-core/tests/prompt_tier_contract.rs` reads this file
//! and the gateway's renderer as source and fails when a block is placed ahead
//! of an earlier tier or has no tier; it cannot depend on the gateway, which is
//! why the rule lives here as text both can be held to.

use std::ops::Range;

/// Where a prompt block sits, by how often it changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PromptTier {
    Stable,
    Context,
    Conversation,
    Volatile,
    Current,
}

impl PromptTier {
    /// Every tier, in prompt order.
    pub const ALL: [PromptTier; 5] = [
        PromptTier::Stable,
        PromptTier::Context,
        PromptTier::Conversation,
        PromptTier::Volatile,
        PromptTier::Current,
    ];

    /// The closed token for logs and metrics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Context => "context",
            Self::Conversation => "conversation",
            Self::Volatile => "volatile",
            Self::Current => "current",
        }
    }
}

/// How many leading messages of a prompt are Stable and how many Context
/// follow them. These two tiers are the ones a provider can cache across
/// turns, so they are the only ones a plan names: everything after them is
/// whatever the turn made it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PromptTierPlan {
    /// Messages in the Stable tier, from index 0.
    pub stable: usize,
    /// Messages in the Context tier, directly after the Stable ones.
    pub context: usize,
}

/// The plan of a prompt whose messages carry `tiers`, in order.
///
/// Counts the leading Stable run and the Context run after it. A Stable or
/// Context message that comes after a later tier is not counted: it is not
/// part of a prefix, and a plan must never promise a prefix the prompt does
/// not have.
#[must_use]
pub fn plan_of(tiers: &[PromptTier]) -> PromptTierPlan {
    let stable = tiers
        .iter()
        .take_while(|tier| **tier == PromptTier::Stable)
        .count();
    let context = tiers[stable..]
        .iter()
        .take_while(|tier| **tier == PromptTier::Context)
        .count();
    PromptTierPlan { stable, context }
}

/// The message ranges a plan names, in prompt order, empty tiers left out.
/// A consumer that marks cache breakpoints walks these instead of redoing the
/// arithmetic.
#[must_use]
pub fn blocks_of(plan: PromptTierPlan) -> Vec<(PromptTier, Range<usize>)> {
    let context_end = plan.stable + plan.context;
    [
        (PromptTier::Stable, 0..plan.stable),
        (PromptTier::Context, plan.stable..context_end),
    ]
    .into_iter()
    .filter(|(_, range)| !range.is_empty())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use PromptTier::{Context, Conversation, Current, Stable, Volatile};

    #[test]
    fn tiers_order_from_stable_to_current() {
        assert!(PromptTier::ALL.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(Stable < Context && Context < Conversation);
        assert!(Conversation < Volatile && Volatile < Current);
    }

    #[test]
    fn tokens_are_distinct_and_closed() {
        let tokens: Vec<&str> = PromptTier::ALL.iter().map(|tier| tier.as_str()).collect();
        assert_eq!(
            tokens,
            ["stable", "context", "conversation", "volatile", "current"]
        );
    }

    #[test]
    fn a_plan_counts_the_leading_stable_and_context_runs() {
        let tiers = [
            Stable,
            Stable,
            Context,
            Context,
            Context,
            Conversation,
            Conversation,
            Volatile,
            Current,
        ];
        assert_eq!(
            plan_of(&tiers),
            PromptTierPlan {
                stable: 2,
                context: 3
            }
        );
    }

    #[test]
    fn a_prompt_without_standing_blocks_has_an_empty_plan() {
        assert_eq!(plan_of(&[]), PromptTierPlan::default());
        assert_eq!(
            plan_of(&[Conversation, Volatile, Current]),
            PromptTierPlan::default()
        );
    }

    #[test]
    fn context_without_stable_is_still_a_prefix() {
        assert_eq!(
            plan_of(&[Context, Conversation, Current]),
            PromptTierPlan {
                stable: 0,
                context: 1
            }
        );
    }

    #[test]
    fn a_standing_block_after_a_later_tier_is_not_counted() {
        // Stable after Conversation is an ordering bug; the plan must not
        // promise it as part of a prefix.
        assert_eq!(
            plan_of(&[Stable, Conversation, Stable, Context, Current]),
            PromptTierPlan {
                stable: 1,
                context: 0
            }
        );
        assert_eq!(
            plan_of(&[Stable, Context, Volatile, Context]),
            PromptTierPlan {
                stable: 1,
                context: 1
            }
        );
    }

    #[test]
    fn blocks_cover_exactly_the_planned_messages() {
        let plan = PromptTierPlan {
            stable: 2,
            context: 3,
        };
        assert_eq!(blocks_of(plan), vec![(Stable, 0..2), (Context, 2..5)]);
    }

    #[test]
    fn blocks_leave_empty_tiers_out() {
        assert!(blocks_of(PromptTierPlan::default()).is_empty());
        assert_eq!(
            blocks_of(PromptTierPlan {
                stable: 0,
                context: 2
            }),
            vec![(Context, 0..2)]
        );
        assert_eq!(
            blocks_of(PromptTierPlan {
                stable: 1,
                context: 0
            }),
            vec![(Stable, 0..1)]
        );
    }

    #[test]
    fn blocks_and_plan_round_trip() {
        let tiers = [Stable, Context, Context, Conversation, Current];
        let plan = plan_of(&tiers);
        for (tier, range) in blocks_of(plan) {
            assert!(tiers[range].iter().all(|found| *found == tier));
        }
    }
}
