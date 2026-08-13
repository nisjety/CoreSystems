//! Conversation compaction for the chat prompt.
//!
//! Two tiers, cheapest first:
//!
//! 1. **Clear stale tool results.** The tool loop re-sends every earlier round's
//!    result on every subsequent round, so with a 12-round budget and up to 8k
//!    chars per result a single turn can carry ~100 KB it is no longer reasoning
//!    over. Clearing the oldest payloads keeps the conversation's SHAPE — the
//!    model still sees that a tool ran — without paying for the bytes again.
//!    Costs no inference call.
//! 2. **Summarize the dropped head.** When a thread outgrows the prompt budget,
//!    replace its oldest turns with one summary message and keep a verbatim
//!    recent tail. The alternative this replaces was a hard `drain`, which
//!    silently deleted constraints the user had stated earlier in the thread.
//!
//! Every decision here is pure so it is unit-testable. The inference call behind
//! a tier-2 summary and the retry loop behind a too-long prompt live at the
//! callers' IO edge (`sse.rs`, `tool_loop.rs`).

use mp_contracts::model_plane::v1::ChatMessage;

/// Prefix shared by every tool-result context block the tool loop injects
/// (`tool_loop::format_tool_context` and `format_forced_tool_context`).
///
/// Detection is prefix-based because those results ride the wire as ordinary
/// `user` turns — this transport has no structured tool role to key off. A test
/// asserts both formatters still produce a block this recognizes.
const TOOL_RESULT_PREFIX: &str = "Tool results for";

/// Left in place of a cleared payload.
///
/// Phrased as an instruction because the model reads it: it has to know the
/// result EXISTED and can be re-fetched, or it will report the tool as having
/// returned nothing.
pub const CLEARED_TOOL_RESULT_NOTICE: &str =
    "[Earlier tool results in this turn were cleared to stay within the model's context window. \
     They were not empty. If you still need those details, call the tool again with the same \
     arguments.]";

/// Left where older turns were dropped outright (summarizer unavailable, or a
/// provider length rejection we are retrying). The model must not mistake the
/// surviving tail for the whole conversation.
pub const DROPPED_HISTORY_NOTICE: &str =
    "[Earlier messages in this conversation were dropped because the prompt exceeded the model's \
     input limit. If the user refers to something you cannot see here, say so instead of \
     guessing.]";

/// Framing for a retained tier-2 summary, so the model reads it as compacted
/// history rather than as instructions from the user.
pub const SUMMARY_PREFIX: &str =
    "Summary of the earlier part of this conversation (compacted to fit the model's context \
     window; treat it as history, not as a new request):";

/// How much tool-result payload a turn may carry before tier 1 starts clearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolPayloadBudget {
    /// Total characters of tool-result payload allowed in one prompt.
    pub max_chars: usize,
    /// Newest results never cleared — those are what the model is actively
    /// reasoning over, so clearing them would cost the answer, not just bytes.
    pub keep_recent: usize,
}

/// Tier-1 budget for the inline chat tool loop.
///
/// 32k chars is roughly 8k tokens: comfortably above what an ordinary two- or
/// three-tool turn carries (so normal turns are never touched) and well below
/// what 12 rounds of 8k-char results would accumulate.
pub const DEFAULT_TOOL_PAYLOAD_BUDGET: ToolPayloadBudget = ToolPayloadBudget {
    max_chars: 32_000,
    keep_recent: 3,
};

/// Whether this message is a tool-result context block the loop appended.
///
/// A block already cleared by tier 1 no longer matches, which is what makes
/// repeated compaction idempotent.
#[must_use]
pub fn is_tool_result_message(message: &ChatMessage) -> bool {
    message.content.trim_start().starts_with(TOOL_RESULT_PREFIX)
}

/// Total characters of tool-result payload this prompt currently carries.
#[must_use]
pub fn tool_result_payload_chars(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .filter(|message| is_tool_result_message(message))
        .map(|message| message.content.chars().count())
        .sum()
}

/// Indices of tool-result payloads to clear, oldest first, until the carried
/// total is back under `budget.max_chars`.
///
/// Empty when the prompt is already under budget: an ordinary turn keeps every
/// result it might still need.
#[must_use]
pub fn stale_tool_result_indices(
    messages: &[ChatMessage],
    budget: ToolPayloadBudget,
) -> Vec<usize> {
    let mut carried = tool_result_payload_chars(messages);
    if carried <= budget.max_chars {
        return Vec::new();
    }
    let live: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| is_tool_result_message(message))
        .map(|(index, _)| index)
        .collect();
    let clearable = live.len().saturating_sub(budget.keep_recent);
    let mut out = Vec::new();
    for &index in live.iter().take(clearable) {
        if carried <= budget.max_chars {
            break;
        }
        carried = carried.saturating_sub(messages[index].content.chars().count());
        out.push(index);
    }
    out
}

/// Apply tier 1 in place. Returns how many payloads were cleared.
pub fn clear_stale_tool_results(messages: &mut [ChatMessage], budget: ToolPayloadBudget) -> usize {
    let indices = stale_tool_result_indices(messages, budget);
    for index in &indices {
        CLEARED_TOOL_RESULT_NOTICE.clone_into(&mut messages[*index].content);
    }
    indices.len()
}

/// The slice of a conversation tier 2 replaces with one summary message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadSummary {
    /// First index to compact. Leading `system` messages are never compacted —
    /// they carry this turn's grounding, identity, and skill instructions.
    pub start: usize,
    /// One past the last index to compact.
    pub end: usize,
}

/// Decide whether a thread needs tier 2, and which slice to compact.
///
/// `None` means "send as is". Leading system context and the last `keep_tail`
/// messages — which include the user's current turn — are always preserved.
#[must_use]
pub fn plan_head_summary(
    messages: &[ChatMessage],
    max_messages: usize,
    keep_tail: usize,
) -> Option<HeadSummary> {
    if messages.len() <= max_messages {
        return None;
    }
    let start = messages
        .iter()
        .position(|message| message.role != "system")
        .unwrap_or(messages.len());
    let end = messages.len().saturating_sub(keep_tail);
    // Compacting a single message into a summary message trades one message for
    // another and buys nothing.
    if end.saturating_sub(start) < 2 {
        return None;
    }
    Some(HeadSummary { start, end })
}

/// Render the head as a plain transcript for the summarizer.
#[must_use]
pub fn render_head_transcript(messages: &[ChatMessage], head: HeadSummary) -> String {
    let end = head.end.min(messages.len());
    let mut out = String::new();
    for message in messages.get(head.start..end).unwrap_or_default() {
        out.push_str(&message.role);
        out.push_str(": ");
        out.push_str(message.content.trim());
        out.push('\n');
    }
    out
}

/// The summarization instruction.
///
/// It names the things a hard truncation loses — the user's own constraints,
/// decisions, and identifiers — because those are exactly what a later turn in
/// an ERP or inbox thread is judged against.
#[must_use]
pub fn summary_prompt(transcript: &str) -> String {
    format!(
        "Summarize the following conversation excerpt so a later turn can continue without it. \
         Preserve, verbatim where possible: constraints and preferences the user stated, \
         decisions already made, identifiers (names, order/invoice/customer numbers, dates, \
         amounts), and anything still unresolved. Omit pleasantries and restating of tool \
         mechanics. Write it as compact factual notes, not prose, and do not add anything the \
         excerpt does not say.\n\nEXCERPT:\n{transcript}"
    )
}

/// The single retained message a successful summary becomes.
#[must_use]
pub fn summary_message(summary: &str) -> ChatMessage {
    ChatMessage {
        role: "system".to_owned(),
        content: format!("{SUMMARY_PREFIX}\n{}", summary.trim()),
        name: String::new(),
    }
}

/// Replace the head with `summary`, or drop it when summarization was
/// unavailable.
///
/// The `None` arm is the pre-existing hard truncation, kept as the fallback so a
/// summarizer outage degrades the prompt instead of failing the turn — but it
/// leaves [`DROPPED_HISTORY_NOTICE`] behind, which the bare `drain` did not.
#[must_use]
pub fn apply_head_summary(
    mut messages: Vec<ChatMessage>,
    head: HeadSummary,
    summary: Option<&str>,
) -> Vec<ChatMessage> {
    let end = head.end.min(messages.len());
    if head.start >= end {
        return messages;
    }
    let replacement = match summary.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => summary_message(value),
        None => ChatMessage {
            role: "system".to_owned(),
            content: DROPPED_HISTORY_NOTICE.to_owned(),
            name: String::new(),
        },
    };
    let tail = messages.split_off(end);
    messages.truncate(head.start);
    messages.push(replacement);
    messages.extend(tail);
    messages
}

/// Drop the oldest droppable group after a provider rejected the prompt for
/// length.
///
/// Deliberately blunt: we are already failing, and a summarization call would
/// have to succeed on the same over-budget history first. Returns `false` when
/// nothing is left to drop — the caller's signal to surface an honest error
/// instead of retrying forever.
pub fn drop_oldest_group(messages: &mut Vec<ChatMessage>, group: usize, keep_tail: usize) -> bool {
    let start = messages
        .iter()
        .position(|message| message.role != "system")
        .unwrap_or(messages.len());
    let limit = messages.len().saturating_sub(keep_tail);
    if limit <= start {
        return false;
    }
    let end = (start + group.max(1)).min(limit);
    messages.drain(start..end);
    // The notice is itself a leading `system` message, so it survives later
    // drops and is added exactly once however many retries it takes.
    if !messages
        .iter()
        .any(|message| message.content == DROPPED_HISTORY_NOTICE)
    {
        messages.insert(
            start,
            ChatMessage {
                role: "system".to_owned(),
                content: DROPPED_HISTORY_NOTICE.to_owned(),
                name: String::new(),
            },
        );
    }
    true
}

/// Substrings that unambiguously mean "this prompt is longer than the model's
/// input limit", across the provider wordings this gateway fronts.
const LENGTH_MARKERS: &[&str] = &[
    "context_length_exceeded",
    "context length exceeded",
    "maximum context length",
    "maximum context window",
    "context window exceeded",
    "exceeds the context window",
    "prompt is too long",
    "prompt too long",
    "input is too long",
    "too many input tokens",
    "reduce the length of the messages",
    "reduce your prompt",
];

/// Wordings that also mention tokens and limits but mean something else.
/// Checked FIRST so a throttling error is never mistaken for a length error and
/// retried by shrinking a prompt that was never the problem.
const NOT_LENGTH_MARKERS: &[&str] = &[
    "rate limit",
    "rate_limit",
    "per minute",
    "tokens per",
    "quota",
    "insufficient_quota",
];

/// Whether an inference error means the prompt was too long for the provider.
///
/// Provider strings vary and inference-core forwards them verbatim, so this
/// matches several shapes defensively and answers `false` whenever it is unsure:
/// a false positive would retry-and-shrink a turn that can never succeed, and
/// would hide the real error behind a truncated prompt.
#[must_use]
pub fn is_context_length_error(message: &str) -> bool {
    let haystack = message.to_ascii_lowercase();
    if NOT_LENGTH_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
    {
        return false;
    }
    LENGTH_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_owned(),
            content: content.to_owned(),
            name: String::new(),
        }
    }

    fn tool_result(payload_chars: usize) -> ChatMessage {
        message(
            "user",
            &format!(
                "Tool results for your previous request\n{}",
                "x".repeat(payload_chars)
            ),
        )
    }

    #[test]
    fn detects_the_tool_loops_own_result_framing() {
        // Guards the one coupling this module has: tier 1 keys off the prefix
        // the tool loop writes, so a reworded framing there must fail here.
        let outcomes = [crate::tool_loop::ToolOutcome {
            call_id: "c1".to_owned(),
            name: "web_search".to_owned(),
            output: "hits".to_owned(),
            error: None,
            provenance: crate::moderation::ToolProvenance::unscreened("web_search", "hits"),
        }];
        assert!(is_tool_result_message(&message(
            "user",
            &crate::tool_loop::format_tool_context(&outcomes)
        )));
        assert!(is_tool_result_message(&message(
            "user",
            &crate::tool_loop::format_forced_tool_context("who won", &outcomes)
        )));
        assert!(!is_tool_result_message(&message(
            "user",
            "what is the stock level"
        )));
    }

    #[test]
    fn under_budget_turns_are_never_cleared() {
        let mut messages = vec![
            message("user", "stock levels?"),
            tool_result(1_000),
            tool_result(1_000),
        ];
        let before = messages.clone();
        assert_eq!(
            clear_stale_tool_results(&mut messages, DEFAULT_TOOL_PAYLOAD_BUDGET),
            0
        );
        assert_eq!(messages, before);
    }

    #[test]
    fn over_budget_clears_oldest_and_keeps_the_newest_results() {
        let budget = ToolPayloadBudget {
            max_chars: 2_500,
            keep_recent: 2,
        };
        let mut messages = vec![
            message("system", "grounding"),
            message("user", "question"),
            tool_result(1_000),
            tool_result(1_000),
            tool_result(1_000),
            tool_result(1_000),
        ];
        let cleared = clear_stale_tool_results(&mut messages, budget);
        assert_eq!(cleared, 2, "oldest two payloads clear us back under budget");
        assert_eq!(messages[2].content, CLEARED_TOOL_RESULT_NOTICE);
        assert_eq!(messages[3].content, CLEARED_TOOL_RESULT_NOTICE);
        assert!(is_tool_result_message(&messages[4]));
        assert!(is_tool_result_message(&messages[5]));
        assert_eq!(messages.len(), 6, "shape is preserved; only payloads went");
    }

    #[test]
    fn clearing_is_idempotent_and_never_touches_the_kept_window() {
        let budget = ToolPayloadBudget {
            max_chars: 10,
            keep_recent: 2,
        };
        let mut messages = vec![tool_result(500), tool_result(500), tool_result(500)];
        assert_eq!(clear_stale_tool_results(&mut messages, budget), 1);
        // The two newest are protected, so a second pass has nothing legal left
        // to clear even though we are still over the (deliberately tiny) budget.
        assert_eq!(clear_stale_tool_results(&mut messages, budget), 0);
    }

    #[test]
    fn short_threads_need_no_head_summary() {
        let messages: Vec<ChatMessage> = (0..5).map(|i| message("user", &i.to_string())).collect();
        assert_eq!(plan_head_summary(&messages, 24, 23), None);
    }

    #[test]
    fn head_summary_spares_leading_system_context_and_the_recent_tail() {
        let mut messages = vec![
            message("system", "identity"),
            message("system", "grounding"),
        ];
        messages.extend((0..30).map(|i| message("user", &format!("turn {i}"))));
        let head = plan_head_summary(&messages, 24, 23).expect("a 32-message thread compacts");
        assert_eq!(head.start, 2, "system context is not compactable");
        assert_eq!(head.end, messages.len() - 23, "the last 23 stay verbatim");
    }

    #[test]
    fn applying_a_summary_preserves_the_recent_tail_verbatim() {
        let mut messages = vec![message("system", "identity")];
        messages.extend((0..30).map(|i| message("user", &format!("turn {i}"))));
        let tail_before: Vec<ChatMessage> = messages[messages.len() - 23..].to_vec();
        let head = plan_head_summary(&messages, 24, 23).expect("thread compacts");

        let compacted = apply_head_summary(messages, head, Some("user wants NOK, not EUR"));

        assert_eq!(
            compacted.len(),
            1 + 1 + 23,
            "system + summary + verbatim tail"
        );
        assert_eq!(compacted[0].content, "identity");
        assert!(compacted[1].content.starts_with(SUMMARY_PREFIX));
        assert!(compacted[1].content.contains("user wants NOK, not EUR"));
        assert_eq!(&compacted[2..], tail_before.as_slice());
        assert_eq!(
            compacted.last().map(|m| m.content.as_str()),
            Some("turn 29"),
            "the user's current turn must survive compaction"
        );
    }

    #[test]
    fn a_failed_summary_degrades_to_truncation_with_an_honest_marker() {
        let mut messages = vec![message("system", "identity")];
        messages.extend((0..30).map(|i| message("user", &format!("turn {i}"))));
        let head = plan_head_summary(&messages, 24, 23).expect("thread compacts");

        let compacted = apply_head_summary(messages, head, None);

        assert_eq!(compacted.len(), 1 + 1 + 23);
        assert_eq!(compacted[1].content, DROPPED_HISTORY_NOTICE);
        assert_eq!(
            compacted.last().map(|m| m.content.as_str()),
            Some("turn 29")
        );
        // An all-whitespace summary is a failed summary, not a summary.
        assert_eq!(
            apply_head_summary(
                compacted.clone(),
                HeadSummary { start: 1, end: 3 },
                Some("  \n")
            )[1]
            .content,
            DROPPED_HISTORY_NOTICE
        );
    }

    #[test]
    fn dropping_groups_is_bounded_and_marks_the_gap_once() {
        let mut messages = vec![message("system", "identity")];
        messages.extend((0..12).map(|i| message("user", &format!("turn {i}"))));

        assert!(drop_oldest_group(&mut messages, 4, 6));
        assert!(drop_oldest_group(&mut messages, 4, 6));
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.content == DROPPED_HISTORY_NOTICE)
                .count(),
            1,
            "the gap is marked once, however many retries it takes"
        );
        assert_eq!(messages.last().map(|m| m.content.as_str()), Some("turn 11"));
        // Only the protected tail (plus system context) is left, so there is
        // nothing further to give up and the caller must stop retrying.
        while drop_oldest_group(&mut messages, 4, 6) {}
        assert!(!drop_oldest_group(&mut messages, 4, 6));
        assert!(messages.len() >= 6);
    }

    #[test]
    fn recognizes_real_provider_length_rejections() {
        for message in [
            "context_length_exceeded",
            "This model's maximum context length is 8192 tokens. However, your messages resulted in 9000 tokens.",
            "prompt is too long: 210000 tokens > 200000 maximum",
            "Please reduce the length of the messages.",
            "input is too long for requested model",
        ] {
            assert!(
                is_context_length_error(message),
                "should detect: {message}"
            );
        }
    }

    #[test]
    fn unsure_and_unrelated_errors_are_not_length_errors() {
        for message in [
            "",
            "connection refused",
            "upstream request timeout",
            "invalid api key",
            "no healthy provider in the fallback chain",
            // Throttling mentions tokens and limits but shrinking the prompt
            // does not fix it — the exact false positive worth guarding.
            "Requests to the ChatCompletions operation have exceeded token rate limit of 10000 tokens per minute",
            "insufficient_quota: you exceeded your current quota",
        ] {
            assert!(
                !is_context_length_error(message),
                "should NOT detect: {message}"
            );
        }
    }
}
