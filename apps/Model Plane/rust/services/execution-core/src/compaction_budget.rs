//! Tier-1 compaction for the governed agent loop: clear stale tool-result
//! payloads before they crowd out the conversation.
//!
//! # Why this exists at all
//!
//! The inline chat loop has had tiered compaction for a while
//! (`model-gateway::compaction`). This loop had **none** — and it is the one
//! that needed it more: a chat turn carries two or three tool results, while a
//! twelve-round agentic run accumulates every round's payload and re-sends the
//! lot on every later round. An overflowing agentic run simply failed
//! (`RoundsOutcome::InferFailed` → the graceful-failure sentence), while
//! `StepOutcome.compaction_triggered` was surfaced over gRPC as a hardcoded
//! `false`.
//!
//! # Why a second implementation and not a shared crate
//!
//! model-gateway and execution-core deploy separately and neither may depend on
//! the other, which is the same situation `runtime_loop::retry` and
//! `skill_budget` are in. The established answer here is: implement in both,
//! keep the CONSTANTS and the notice text identical, and pin them with a
//! contract test that reads the other service's source
//! (`tests/compaction_parity_contract.rs`). A deployed agent that compacts at a
//! different threshold than chat, or leaves a different notice behind, answers
//! differently for reasons nobody can trace.
//!
//! # What the notice has to do
//!
//! It is phrased as an instruction because the model reads it and must not
//! conclude the tool returned nothing. Clearing a payload is lossy; saying so,
//! and saying the result can be re-fetched, is what keeps a compacted run honest
//! rather than merely smaller.

use mp_contracts::model_plane::v1::ChatMessage;

/// Prefix `format_tool_context` writes, and the only way to recognise a
/// tool-result block on this transport — these ride the wire as ordinary `user`
/// turns, with no structured tool role to key off.
///
/// Kept identical to model-gateway's `TOOL_RESULT_PREFIX`. A test asserts this
/// loop's own formatter still produces a block this matches, so the detector
/// cannot drift away from the thing it detects.
const TOOL_RESULT_PREFIX: &str = "Tool results for";

/// Left in place of a cleared payload.
///
/// Byte-identical to model-gateway's `CLEARED_TOOL_RESULT_NOTICE`, pinned by the
/// parity contract test: two loops leaving different notices means the same
/// compaction reads as a different event depending on which surface ran it.
pub(crate) const CLEARED_TOOL_RESULT_NOTICE: &str =
    "[Earlier tool results in this turn were cleared to stay within the model's context window. \
     They were not empty. If you still need those details, call the tool again with the same \
     arguments.]";

/// Total characters of tool-result payload one prompt may carry before clearing
/// starts. Identical to model-gateway's `DEFAULT_TOOL_PAYLOAD_BUDGET.max_chars`.
pub(crate) const MAX_TOOL_PAYLOAD_CHARS: usize = 32_000;

/// Newest payloads never cleared — those are what the model is actively
/// reasoning over, so clearing them costs the answer rather than just bytes.
/// Identical to model-gateway's `keep_recent`.
pub(crate) const KEEP_RECENT_PAYLOADS: usize = 3;

/// Whether this message is a tool-result block the loop appended.
///
/// A block already cleared no longer matches, which is what makes repeated
/// compaction idempotent.
#[must_use]
pub(crate) fn is_tool_result_message(message: &ChatMessage) -> bool {
    message.content.trim_start().starts_with(TOOL_RESULT_PREFIX)
}

/// Characters of tool-result payload this prompt currently carries.
#[must_use]
pub(crate) fn tool_result_payload_chars(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .filter(|message| is_tool_result_message(message))
        .map(|message| message.content.chars().count())
        .sum()
}

/// Indices to clear, oldest first, until the carried total is back under budget.
///
/// Empty when already under budget: an ordinary run keeps every result it might
/// still need, which is why this can run unconditionally every round.
#[must_use]
pub(crate) fn stale_tool_result_indices(messages: &[ChatMessage]) -> Vec<usize> {
    let mut carried = tool_result_payload_chars(messages);
    if carried <= MAX_TOOL_PAYLOAD_CHARS {
        return Vec::new();
    }
    let live: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| is_tool_result_message(message))
        .map(|(index, _)| index)
        .collect();
    let clearable = live.len().saturating_sub(KEEP_RECENT_PAYLOADS);
    let mut out = Vec::new();
    for &index in live.iter().take(clearable) {
        if carried <= MAX_TOOL_PAYLOAD_CHARS {
            break;
        }
        carried = carried.saturating_sub(messages[index].content.chars().count());
        out.push(index);
    }
    out
}

/// Apply tier 1 in place. Returns how many payloads were cleared.
pub(crate) fn clear_stale_tool_results(messages: &mut [ChatMessage]) -> usize {
    let indices = stale_tool_result_indices(messages);
    for index in &indices {
        CLEARED_TOOL_RESULT_NOTICE.clone_into(&mut messages[*index].content);
    }
    indices.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_result(chars: usize) -> ChatMessage {
        ChatMessage {
            role: "user".to_owned(),
            content: format!(
                "{TOOL_RESULT_PREFIX} your previous request:\n{}",
                "x".repeat(chars)
            ),
            name: String::new(),
        }
    }

    fn user(content: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_owned(),
            content: content.to_owned(),
            name: String::new(),
        }
    }

    /// This runs every round, so the common case has to be free. An ordinary run
    /// must keep every result it might still be reasoning over.
    #[test]
    fn a_run_under_budget_is_untouched() {
        let mut messages = vec![user("the goal"), tool_result(1_000), tool_result(1_000)];
        let before = messages.clone();
        assert_eq!(clear_stale_tool_results(&mut messages), 0);
        assert_eq!(messages, before);
    }

    /// Oldest first, and never the newest few — those are what the model is
    /// actively using, so clearing them costs the answer, not just bytes.
    #[test]
    fn clearing_starts_with_the_oldest_and_spares_the_newest() {
        let mut messages: Vec<ChatMessage> = (0..8).map(|_| tool_result(9_000)).collect();
        let cleared = clear_stale_tool_results(&mut messages);
        assert!(cleared > 0, "72k of payload is over the 32k budget");

        let kept_tail = &messages[messages.len() - KEEP_RECENT_PAYLOADS..];
        for message in kept_tail {
            assert_ne!(
                message.content, CLEARED_TOOL_RESULT_NOTICE,
                "the newest {KEEP_RECENT_PAYLOADS} payloads must survive"
            );
        }
        assert_eq!(
            messages[0].content, CLEARED_TOOL_RESULT_NOTICE,
            "the oldest goes first"
        );
    }

    /// The budget is a ceiling, not a suggestion — over any input shape.
    #[test]
    fn clearing_brings_the_carried_total_under_budget() {
        for (count, size) in [(8usize, 9_000usize), (40, 2_000), (4, 30_000)] {
            let mut messages: Vec<ChatMessage> = (0..count).map(|_| tool_result(size)).collect();
            clear_stale_tool_results(&mut messages);
            let carried = tool_result_payload_chars(&messages);
            // The spared tail can exceed the budget on its own — clearing it
            // would cost the answer — so the guarantee is "cleared everything
            // clearable", not "always under".
            let clearable = count.saturating_sub(KEEP_RECENT_PAYLOADS);
            let tail_only = clearable * CLEARED_TOOL_RESULT_NOTICE.chars().count()
                + KEEP_RECENT_PAYLOADS.min(count) * size;
            assert!(
                carried <= MAX_TOOL_PAYLOAD_CHARS || carried <= tail_only,
                "{count}x{size}: carried {carried} after clearing"
            );
        }
    }

    /// Compaction has to be safe to run repeatedly: a cleared block no longer
    /// matches the prefix, so a second pass finds nothing to do.
    #[test]
    fn a_second_pass_clears_nothing_more() {
        let mut messages: Vec<ChatMessage> = (0..8).map(|_| tool_result(9_000)).collect();
        let first = clear_stale_tool_results(&mut messages);
        assert!(first > 0);
        assert_eq!(
            clear_stale_tool_results(&mut messages),
            0,
            "clearing must be idempotent, or every round would re-clear"
        );
    }

    /// Only tool results. A cleared user turn or system block would lose the
    /// goal or the grounding this run was given.
    #[test]
    fn nothing_but_tool_results_is_ever_cleared() {
        let mut messages = vec![
            user("the goal, which must survive"),
            tool_result(40_000),
            tool_result(40_000),
            user("a later instruction"),
        ];
        clear_stale_tool_results(&mut messages);
        assert_eq!(messages[0].content, "the goal, which must survive");
        assert_eq!(messages[3].content, "a later instruction");
    }

    /// The notice is what keeps a compacted run honest: it has to say the result
    /// existed and can be re-fetched, or the model reports the tool as having
    /// returned nothing.
    #[test]
    fn the_notice_says_the_result_existed_and_can_be_refetched() {
        let notice = CLEARED_TOOL_RESULT_NOTICE.to_lowercase();
        assert!(notice.contains("not empty"));
        assert!(notice.contains("call the tool again"));
    }
}
