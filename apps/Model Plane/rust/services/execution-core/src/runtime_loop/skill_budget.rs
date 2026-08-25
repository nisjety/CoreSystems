//! Character budget for injected skill guidance — degrade before dropping.
//!
//! Skill injection was capped by COUNT (`MAX_INJECTED_SKILLS`) and not by size.
//! A skill's `content` is operator-authored free text with no length limit, so
//! three long skills could occupy more of the prompt than the conversation they
//! were meant to steer — and the cost is invisible: nothing fails, the model
//! just has less room for history and answers worse.
//!
//! # Degrade before drop
//!
//! When the budget runs out the obvious move is to drop the remaining skills.
//! Truncating instead keeps the higher-scoring skill's opening guidance, which
//! is where operators put the important part, and keeps the model aware the
//! skill exists at all.
//!
//! But a silently truncated instruction is worse than a dropped one: the model
//! acts on half a rule believing it is whole. So every truncated block carries
//! an explicit marker, and any skill that cannot keep at least
//! [`MIN_SKILL_CHARS`] is dropped rather than reduced to a title and a stub —
//! below that threshold a block is noise that costs tokens and teaches nothing.
//! Dropped skills are reported, never silent.

/// Total characters skill guidance may occupy in one prompt.
///
/// Roughly 1% of a 200k-token window (~2k tokens at ~4 chars/token). Chosen as a
/// character budget rather than a token one deliberately: the model — and
/// therefore the real window — is resolved downstream by inference-core's intent
/// layer, so at this point in the code any token figure would be a guess
/// dressed as a measurement. Characters are what we can actually count here.
pub(crate) const SKILL_CONTEXT_BUDGET_CHARS: usize = 8_000;

/// Least content a truncated skill may keep and still be worth injecting.
pub(crate) const MIN_SKILL_CHARS: usize = 240;

/// Appended to a skill whose content was cut, so the model knows the rule it is
/// reading is incomplete instead of acting on half of it as if it were whole.
pub(crate) const TRUNCATION_MARKER: &str =
    "\n[… skill truncated to fit the context budget. Call reattach_skill with this \
     skill's name to read the rest before acting on it.]";

/// The outcome of fitting blocks into the budget.
pub(crate) struct FittedSkills {
    /// Ready-to-inject blocks, in the order given.
    pub(crate) blocks: Vec<String>,
    /// How many blocks were truncated to fit.
    pub(crate) truncated: usize,
    /// How many were dropped entirely for want of room.
    pub(crate) dropped: usize,
}

/// Fit already-formatted skill blocks into [`SKILL_CONTEXT_BUDGET_CHARS`],
/// truncating before dropping and never emitting a block below
/// [`MIN_SKILL_CHARS`].
///
/// Takes formatted blocks rather than name/content pairs so the two loops can
/// share one budget without sharing a block format: execution-core writes
/// `## Skill: <name>` and model-gateway has its own `format_skill_block`. What
/// must agree is the BUDGET, not the layout.
///
/// Callers pass blocks already ordered by relevance: the budget is spent front
/// to back, so ordering decides what survives.
pub(crate) fn fit_skill_blocks(blocks: Vec<String>) -> FittedSkills {
    let mut kept = Vec::new();
    let mut truncated = 0usize;
    let mut dropped = 0usize;
    let mut remaining = SKILL_CONTEXT_BUDGET_CHARS;
    let marker_len = TRUNCATION_MARKER.chars().count();

    for block in blocks {
        let len = block.chars().count();
        if len <= remaining {
            remaining -= len;
            kept.push(block);
            continue;
        }
        // Not enough room for a useful remnant plus its marker: drop rather than
        // inject a stub that costs tokens and teaches nothing.
        if remaining <= marker_len + MIN_SKILL_CHARS {
            dropped += 1;
            continue;
        }
        let keep = remaining - marker_len;
        // `chars().take()` cannot split a multi-byte character.
        let head: String = block.chars().take(keep).collect();
        remaining = 0;
        truncated += 1;
        kept.push(format!("{head}{TRUNCATION_MARKER}"));
    }

    FittedSkills {
        blocks: kept,
        truncated,
        dropped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(name: &str, len: usize) -> String {
        format!("## Skill: {name}\n{}", "x".repeat(len))
    }

    fn total_chars(fitted: &FittedSkills) -> usize {
        fitted.blocks.iter().map(|b| b.chars().count()).sum()
    }

    #[test]
    fn small_blocks_pass_through_untouched() {
        let fitted = fit_skill_blocks(vec![block("A", 100), block("B", 200)]);
        assert_eq!(fitted.blocks.len(), 2);
        assert_eq!(fitted.truncated, 0);
        assert_eq!(fitted.dropped, 0);
        assert!(fitted.blocks.iter().all(|b| !b.contains(TRUNCATION_MARKER)));
    }

    /// The invariant: whatever the input, the injected text fits the budget.
    #[test]
    fn the_budget_is_never_exceeded() {
        for sizes in [
            vec![10_000],
            vec![5_000, 5_000, 5_000],
            vec![100; 200],
            vec![SKILL_CONTEXT_BUDGET_CHARS * 3],
        ] {
            let blocks = sizes
                .iter()
                .enumerate()
                .map(|(i, len)| block(&format!("S{i}"), *len))
                .collect();
            let fitted = fit_skill_blocks(blocks);
            assert!(
                total_chars(&fitted) <= SKILL_CONTEXT_BUDGET_CHARS,
                "injected {} chars, budget is {SKILL_CONTEXT_BUDGET_CHARS}",
                total_chars(&fitted)
            );
        }
    }

    /// Degrade before drop: one oversized skill is truncated, not discarded.
    #[test]
    fn an_oversized_block_is_truncated_rather_than_dropped() {
        let fitted = fit_skill_blocks(vec![block("Big", SKILL_CONTEXT_BUDGET_CHARS * 2)]);
        assert_eq!(fitted.blocks.len(), 1);
        assert_eq!(fitted.truncated, 1);
        assert_eq!(fitted.dropped, 0);
        assert!(
            fitted.blocks[0].ends_with(TRUNCATION_MARKER),
            "a truncated skill MUST say so — the model would otherwise act on \
             half a rule as if it were whole"
        );
        // The name survives truncation, so the model still knows which skill.
        assert!(fitted.blocks[0].starts_with("## Skill: Big"));
    }

    /// Ordering decides what survives, so the first (highest-scoring) block must
    /// keep its content whole while later ones absorb the shortfall.
    #[test]
    fn the_highest_scoring_block_is_served_first() {
        let fitted = fit_skill_blocks(vec![
            block("First", 1_000),
            block("Second", SKILL_CONTEXT_BUDGET_CHARS),
        ]);
        assert!(fitted.blocks[0].starts_with("## Skill: First\n"));
        assert!(
            !fitted.blocks[0].contains(TRUNCATION_MARKER),
            "the top-scoring skill fits and must not be cut"
        );
        assert_eq!(
            fitted.truncated, 1,
            "the shortfall lands on the later skill"
        );
    }

    /// A block with no useful room left is dropped, not injected as a stub.
    #[test]
    fn blocks_with_no_useful_room_are_dropped_and_counted() {
        let fitted = fit_skill_blocks(vec![
            block("Fills", SKILL_CONTEXT_BUDGET_CHARS),
            block("NoRoom", 5_000),
            block("AlsoNoRoom", 5_000),
        ]);
        assert_eq!(fitted.dropped, 2, "omissions must be counted, never silent");
        assert!(total_chars(&fitted) <= SKILL_CONTEXT_BUDGET_CHARS);
    }

    /// Multi-byte content must not be split mid-character.
    #[test]
    fn truncation_respects_character_boundaries() {
        let fitted = fit_skill_blocks(vec![format!(
            "## Skill: Norsk\n{}",
            "æøå".repeat(SKILL_CONTEXT_BUDGET_CHARS)
        )]);
        assert_eq!(fitted.truncated, 1);
        assert!(fitted.blocks[0].contains('æ'));
        assert!(!fitted.blocks[0].contains('\u{fffd}'));
    }

    #[test]
    fn no_blocks_yields_nothing() {
        let fitted = fit_skill_blocks(Vec::new());
        assert!(fitted.blocks.is_empty());
        assert_eq!(fitted.truncated + fitted.dropped, 0);
    }
}
