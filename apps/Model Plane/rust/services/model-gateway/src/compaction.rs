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
     input limit. They still exist. If the user refers to something you cannot see here, call \
     reattach_context to read it back before answering — and if that returns nothing, say so \
     instead of guessing.]";

/// Framing for a retained tier-2 summary, so the model reads it as compacted
/// history rather than as instructions from the user.
pub const SUMMARY_PREFIX: &str =
    "Summary of the earlier part of this conversation (compacted to fit the model's context \
     window; treat it as history, not as a new request). A summary loses detail: if the user \
     asks about something this summary only gestures at, call reattach_context to read the \
     original messages before answering.";

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
    summary_prompt_with_memory(transcript, "")
}

/// [`summary_prompt`] plus a memory contribution — the `on_pre_compress` hook,
/// adapted from `hermes-agent`'s `MemoryProvider.on_pre_compress` (MIT).
///
/// # Why memory gets a say in what a summary keeps
///
/// Compaction is the one place the system *deliberately* destroys detail, and
/// it decides what to keep knowing only this thread. Memory knows what has
/// mattered to this user before. Without this hook, a standing constraint the
/// user stated three sessions ago — and that memory has durably recorded — is
/// summarized away here on its own merits, because the summarizer has no way
/// to know it is load-bearing.
///
/// `directive` is advisory context for the summarizer, NOT content to copy: an
/// empty or whitespace-only value produces byte-identical output to
/// [`summary_prompt`], so a memory outage degrades to exactly today's
/// behaviour rather than a degraded prompt. The framing tells the model to
/// treat it as a *salience hint about the excerpt*, never as facts to merge in
/// — otherwise a summary of a conversation could start asserting things the
/// conversation never said, which is precisely the fabrication the base prompt
/// already forbids in its last clause.
#[must_use]
pub fn summary_prompt_with_memory(transcript: &str, directive: &str) -> String {
    let base = "Summarize the following conversation excerpt so a later turn can continue \
         without it. Preserve, verbatim where possible: constraints and preferences the user \
         stated, decisions already made, identifiers (names, order/invoice/customer numbers, \
         dates, amounts), and anything still unresolved. Omit pleasantries and restating of \
         tool mechanics. Write it as compact factual notes, not prose, and do not add anything \
         the excerpt does not say.";
    let directive = directive.trim();
    if directive.is_empty() {
        return format!("{base}\n\nEXCERPT:\n{transcript}");
    }
    format!(
        "{base}\n\nThe user has previously established the topics below. If — and only if — \
         the excerpt touches any of them, preserve what it says about them verbatim. These are \
         a hint about what matters, NOT facts to add: never state anything from this list that \
         the excerpt itself does not say.\n\nPREVIOUSLY ESTABLISHED:\n{directive}\
         \n\nEXCERPT:\n{transcript}"
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

/// One loaded conversation message together with the durable id session-core
/// returned for it (`SessionMessage.message_id`).
///
/// The id exists only on the read path; `ChatMessage` — what goes to the
/// provider — has no field for it and should not gain one, because a provider
/// payload is not the place to carry our identifiers.
#[derive(Clone, Debug)]
pub struct IdentifiedMessage {
    pub id: String,
    pub message: ChatMessage,
}

/// Preamble on a hoisted pinned message.
///
/// Phrased so the model does not misread a hoisted turn as something the user
/// is saying *now*: it says what the block is, who chose it, and that it comes
/// from earlier in the same conversation. Without that, a pinned question read
/// as the current question.
const PINNED_MESSAGE_PREAMBLE: &str =
    "[Pinned by the user from earlier in this conversation. It is repeated here      verbatim, out of order, because the user marked it as context that must not      be dropped. Treat it as background, not as the current request.]";

/// Most pinned messages one turn may carry.
///
/// A client-declared list has to be bounded: without a cap, pinning fifty turns
/// would push the live conversation out of the context window through the one
/// path built to protect against exactly that.
pub const MAX_PINNED_MESSAGES: usize = 5;

/// Total characters of pinned content one turn may carry. A single pinned
/// message longer than this is truncated rather than dropped, with the notice
/// below, because silently ignoring a pin the user can see in the UI is worse
/// than shortening it.
pub const MAX_PINNED_CHARS: usize = 8_000;

/// Left where a pinned message was shortened to fit the budget.
const PINNED_TRUNCATION_NOTICE: &str =
    "\n[This pinned message was shortened to fit the context budget.]";

/// Re-express the user's pinned messages as protected leading `system` context
/// — but only the ones that are about to be shed.
///
/// `protect_below` is the exclusive upper index bound of the region load-time
/// compaction will replace (`plan_head_summary`'s `head.end`), or 0 when it will
/// not run. A pinned message at or above that index is left exactly where it is:
/// it is still in the transcript, so it needs no protection, and moving it would
/// change what the model sees for no benefit. The first version of this function
/// hoisted unconditionally, which reordered a twelve-message conversation that
/// was never going to be compacted — an old turn appeared as prominent standing
/// context and was missing from its place in the flow.
///
/// Known limit: this protects against load-time compaction only. A pinned
/// message left in place can still be shed by `drop_oldest_group` on the
/// provider-rejection retry path, which runs later and no longer has the ids
/// needed to recognise it. Protecting that path too means carrying pin identity
/// into `ChatMessage`, which is a provider payload and not the place for our
/// identifiers — so the gap is recorded rather than papered over.
///
/// Returns the messages to send, in order: any hoisted pins first (inside the
/// leading system block, where neither shedder reaches), then everything else
/// with those entries removed so nothing is duplicated.
///
/// Ignores an id that matches nothing. A pin is a SELECTOR, never content: the
/// client names a message that already exists in the durable thread, so a
/// browser cannot use this to inject text it invented as though the user had
/// said it earlier. An id from a locally-created turn that session-core has not
/// persisted yet simply does not match, and the pin starts working once it has.
#[must_use]
pub fn hoist_pinned_messages(
    loaded: Vec<IdentifiedMessage>,
    pinned_ids: &[String],
    protect_below: usize,
) -> Vec<ChatMessage> {
    if pinned_ids.is_empty() || protect_below == 0 {
        return loaded.into_iter().map(|entry| entry.message).collect();
    }
    // Preserve the caller's pin order, not conversation order: the user chose
    // which to pin, and with a cap in play the earliest choices are the ones
    // that must survive.
    let mut wanted: Vec<&String> = Vec::new();
    for id in pinned_ids {
        if id.trim().is_empty() || wanted.iter().any(|seen| *seen == id) {
            continue;
        }
        wanted.push(id);
        if wanted.len() >= MAX_PINNED_MESSAGES {
            break;
        }
    }

    let mut hoisted: Vec<ChatMessage> = Vec::new();
    let mut body: Vec<ChatMessage> = Vec::with_capacity(loaded.len());
    let mut pinned_by_id: Vec<(usize, ChatMessage)> = Vec::new();

    for (index, entry) in loaded.into_iter().enumerate() {
        // Three reasons not to hoist: a `system` message is already protected by
        // both shedders; an entry with no durable id cannot be named by a pin;
        // and a message at or above `protect_below` is not going to be shed, so
        // moving it would change the prompt for nothing.
        let position = if entry.message.role == "system"
            || entry.id.trim().is_empty()
            || index >= protect_below
        {
            None
        } else {
            wanted.iter().position(|id| **id == entry.id)
        };
        match position {
            Some(rank) => pinned_by_id.push((rank, entry.message)),
            None => body.push(entry.message),
        }
    }

    pinned_by_id.sort_by_key(|(rank, _)| *rank);

    let mut budget = MAX_PINNED_CHARS;
    for (_, message) in pinned_by_id {
        if budget == 0 {
            // Out of budget: the message stays in the body where it was, rather
            // than vanishing. It is then droppable like any other turn, which is
            // honest — it was not protected.
            body.insert(0, message);
            continue;
        }
        let trimmed = message.content.trim();
        let (content, shortened) = if trimmed.chars().count() > budget {
            (
                trimmed.chars().take(budget).collect::<String>(),
                true,
            )
        } else {
            (trimmed.to_owned(), false)
        };
        budget = budget.saturating_sub(content.chars().count());
        let mut block = format!("{PINNED_MESSAGE_PREAMBLE}\n{}: {content}", message.role);
        if shortened {
            block.push_str(PINNED_TRUNCATION_NOTICE);
        }
        hoisted.push(ChatMessage {
            role: "system".to_owned(),
            content: block,
            name: String::new(),
        });
    }

    hoisted.extend(body);
    hoisted
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
///
/// # This is the FALLBACK table, not the authoritative one
///
/// inference-core sees the raw provider body and now classifies overflow there
/// (`provider::overflow`), signalling it on the gRPC trailer
/// `x-mp-provider-error: too_long`. Prefer that signal — see
/// [`is_context_length_status`].
///
/// This table remains because a rolling deploy can pair a new gateway with an
/// old inference-core that sends no trailer, and during that window the gateway
/// must still recognise an overflow in order to shed history and retry rather
/// than handing the user a hard error. A contract test pins that this table
/// covers everything the authoritative one knows.
const LENGTH_MARKERS: &[&str] = &[
    "context_length_exceeded",
    "context length exceeded",
    "maximum context length",
    "this model supports at most",
    "reduce the length of the messages",
    "reduce the length of your prompt",
    "reduce your prompt",
    "prompt is too long",
    "prompt too long",
    "input length and `max_tokens` exceed context limit",
    "input length and max_tokens exceed context limit",
    "request_too_large",
    "request too large",
    "maximum context window",
    "context window exceeded",
    "exceeds the context window",
    "exceeds model context",
    "maximum prompt length",
    "prompt exceeds",
    "input is too long",
    "too many input tokens",
    "too many tokens",
    "token limit exceeded",
    "exceeds the maximum number of tokens",
    "reduce the amount of context",
];

/// Wordings that also mention tokens and limits but mean something else.
/// Checked FIRST so a throttling error is never mistaken for a length error and
/// retried by shrinking a prompt that was never the problem.
const NOT_LENGTH_MARKERS: &[&str] = &[
    "rate limit",
    "rate_limit",
    "per minute",
    "per day",
    "tokens per",
    "quota",
    "insufficient_quota",
];

/// Trailer inference-core sets to declare its own classification of a provider
/// failure. Duplicated as a literal rather than imported: model-gateway and
/// inference-core are separately deployed crates with no dependency either way,
/// exactly as with the tool-retry vocabularies. A contract test pins the two
/// spellings together.
const PROVIDER_ERROR_KIND_TRAILER: &str = "x-mp-provider-error";

/// Trailer value meaning "the prompt exceeded the provider's input limit".
const PROVIDER_ERROR_TOO_LONG: &str = "too_long";

/// Whether a failed inference call means the prompt was too long — reading
/// inference-core's typed signal first and falling back to its message text.
///
/// Type before prose: the trailer is set by the only layer that saw the raw
/// provider body, so it cannot be defeated by a provider rewording its error.
/// The text fallback covers a rolling deploy against an older inference-core
/// and is otherwise redundant.
#[must_use]
pub fn is_context_length_status(status: &tonic::Status) -> bool {
    if status
        .metadata()
        .get(PROVIDER_ERROR_KIND_TRAILER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|kind| kind.eq_ignore_ascii_case(PROVIDER_ERROR_TOO_LONG))
    {
        return true;
    }
    is_context_length_error(status.message())
}

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
    fn an_absent_memory_contribution_leaves_the_prompt_byte_identical() {
        // The degradation contract: a memory outage must not change the
        // summarization prompt at all, so compaction quality on a bad day is
        // exactly compaction quality before this hook existed.
        let transcript = "user: hei\nassistant: hallo\n";
        assert_eq!(
            summary_prompt_with_memory(transcript, ""),
            summary_prompt(transcript)
        );
        // Whitespace-only is the same as absent — a backend returning blank
        // entries must not inject an empty "PREVIOUSLY ESTABLISHED:" heading
        // the model would then try to interpret.
        assert_eq!(
            summary_prompt_with_memory(transcript, "  \n\t "),
            summary_prompt(transcript)
        );
    }

    #[test]
    fn a_memory_contribution_is_framed_as_salience_never_as_facts_to_add() {
        // The safety property. A summary that starts asserting things the
        // conversation never said is a fabrication — the base prompt already
        // forbids it, and the memory block must not create a loophole by
        // reading as source material.
        let prompt = summary_prompt_with_memory(
            "user: what about the Bergen order?\n",
            "- prefers invoices in NOK\n",
        );
        assert!(prompt.contains("PREVIOUSLY ESTABLISHED:"));
        assert!(prompt.contains("prefers invoices in NOK"));
        assert!(
            prompt.contains("NOT facts to add"),
            "the block must be explicitly marked as non-source: {prompt}"
        );
        assert!(
            prompt.contains(
                "never state anything from this list that the excerpt itself does not say"
            ),
            "the anti-fabrication instruction must survive: {prompt}"
        );
        // The excerpt still has to be present and clearly delimited from it.
        assert!(prompt.contains("EXCERPT:\nuser: what about the Bergen order?"));
        assert!(
            prompt.find("PREVIOUSLY ESTABLISHED:") < prompt.find("EXCERPT:"),
            "the hint must precede the excerpt, not trail it as an afterthought"
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

    fn identified(id: &str, role: &str, content: &str) -> IdentifiedMessage {
        IdentifiedMessage {
            id: id.to_owned(),
            message: message(role, content),
        }
    }

    /// A conversation long enough for `plan_head_summary` to want to compact it.
    fn long_conversation() -> Vec<IdentifiedMessage> {
        let mut out = vec![identified("", "system", "You are Verevon")];
        for index in 0..40 {
            out.push(identified(
                &format!("m{index}"),
                if index % 2 == 0 { "user" } else { "assistant" },
                &format!("turn {index}"),
            ));
        }
        out
    }

    #[test]
    fn no_pins_leaves_the_conversation_untouched() {
        let loaded = long_conversation();
        let expected: Vec<ChatMessage> =
            loaded.iter().map(|entry| entry.message.clone()).collect();
        assert_eq!(hoist_pinned_messages(loaded, &[], 41), expected);
    }

    #[test]
    fn a_pin_moves_its_message_into_the_leading_system_block() {
        let out = hoist_pinned_messages(long_conversation(), &["m7".to_owned()], 41);
        // Hoisted to the front, as `system`, carrying the original text and its
        // original role.
        assert_eq!(out[0].role, "system");
        assert!(out[0].content.contains("turn 7"));
        assert!(out[0].content.contains("assistant: "));
        assert!(out[0].content.contains("Pinned by the user"));
        // ...and removed from its old position, so the model does not see it twice.
        assert_eq!(
            out.iter().filter(|m| m.content.contains("turn 7")).count(),
            1
        );
        // Nothing else was lost.
        assert_eq!(out.len(), 41);
    }

    /// The feature's actual claim, exercised through the real load-time shedder.
    #[test]
    fn a_pinned_message_survives_head_compaction() {
        let pinned = hoist_pinned_messages(long_conversation(), &["m2".to_owned()], 41);
        let head = plan_head_summary(&pinned, 20, 6).expect("this thread needs compacting");
        let compacted = apply_head_summary(pinned, head, Some("summary of the head"));

        assert!(
            compacted.iter().any(|m| m.content.contains("turn 2")),
            "the pinned turn was summarised away, which is the whole thing a pin prevents"
        );
        // The unpinned head really did go, so the test is not passing because
        // compaction did nothing.
        assert!(!compacted.iter().any(|m| m.content.contains("turn 4")));
    }

    /// The same claim against the provider-rejection shedder.
    #[test]
    fn a_pinned_message_survives_dropping_the_oldest_group() {
        let mut messages = hoist_pinned_messages(long_conversation(), &["m1".to_owned()], 41);
        // Shed until it refuses, the way the retry loop does.
        let mut drops = 0;
        while drop_oldest_group(&mut messages, 8, 4) {
            drops += 1;
            assert!(drops < 20, "drop_oldest_group never terminated");
        }
        assert!(drops > 0, "nothing was dropped, so nothing was proven");
        assert!(
            messages.iter().any(|m| m.content.contains("turn 1")),
            "the pinned turn was shed for length"
        );
    }

    #[test]
    fn an_unknown_id_is_ignored_rather_than_invented() {
        let out = hoist_pinned_messages(long_conversation(), &["not-a-message".to_owned()], 41);
        assert_eq!(out.len(), 41);
        assert!(!out[0].content.contains("Pinned by the user"));
    }

    #[test]
    fn pin_order_is_the_users_order_not_conversation_order() {
        let out = hoist_pinned_messages(
            long_conversation(),
            &["m9".to_owned(), "m3".to_owned()],
            41,
        );
        assert!(out[0].content.contains("turn 9"));
        assert!(out[1].content.contains("turn 3"));
    }

    #[test]
    fn the_pin_count_is_capped() {
        let ids: Vec<String> = (0..MAX_PINNED_MESSAGES + 4)
            .map(|index| format!("m{index}"))
            .collect();
        let out = hoist_pinned_messages(long_conversation(), &ids, 41);
        let hoisted = out
            .iter()
            .filter(|m| m.content.contains("Pinned by the user"))
            .count();
        assert_eq!(hoisted, MAX_PINNED_MESSAGES);
        // The ones over the cap are still in the conversation, just unprotected.
        assert_eq!(out.len(), 41);
    }

    #[test]
    fn an_oversized_pin_is_shortened_rather_than_dropped() {
        let huge = "x".repeat(MAX_PINNED_CHARS + 500);
        let loaded = vec![
            identified("", "system", "You are Verevon"),
            identified("big", "user", &huge),
            identified("m1", "user", "later turn"),
        ];
        let out = hoist_pinned_messages(loaded, &["big".to_owned()], 3);
        assert!(out[0].content.contains("Pinned by the user"));
        assert!(out[0].content.contains("was shortened"));
        assert!(out[0].content.chars().count() < MAX_PINNED_CHARS + 500);
    }

    #[test]
    fn a_duplicate_id_is_pinned_once() {
        let out = hoist_pinned_messages(
            long_conversation(),
            &["m5".to_owned(), "m5".to_owned()],
            41,
        );
        assert_eq!(
            out.iter()
                .filter(|m| m.content.contains("Pinned by the user"))
                .count(),
            1
        );
    }

    /// A `system` turn is already protected by both shedders, so hoisting one
    /// would duplicate it and buy nothing.
    #[test]
    fn pinning_a_system_message_is_a_no_op() {
        let loaded = vec![
            identified("sys", "system", "You are Verevon"),
            identified("m1", "user", "hello"),
        ];
        let out = hoist_pinned_messages(loaded, &["sys".to_owned()], 2);
        assert_eq!(out.len(), 2);
        assert!(!out[0].content.contains("Pinned by the user"));
    }

    /// An id is a SELECTOR. A message with no durable id yet (a turn
    /// session-core has not persisted) cannot be pinned by sending an empty id,
    /// and an empty id must not match the empty-id rows either.
    #[test]
    fn an_empty_id_matches_nothing() {
        let loaded = vec![
            identified("", "user", "not yet persisted"),
            identified("m1", "user", "persisted"),
        ];
        let out = hoist_pinned_messages(loaded, &[String::new(), "  ".to_owned()], 2);
        assert_eq!(out.len(), 2);
        assert!(!out[0].content.contains("Pinned by the user"));
    }
    /// The case that was actually reported. A pin on a conversation short enough
    /// that nothing will be compacted must leave the prompt exactly as it was:
    /// no hoisting, no reordering, no turn missing from the flow. The first
    /// version hoisted regardless, so an old answer showed up as prominent
    /// standing context on a twelve-message thread and its turn was gone from
    /// its place — which is what made "summarise what you just said" read wrong.
    #[test]
    fn a_short_conversation_is_never_reordered() {
        let loaded = vec![
            identified("", "system", "You are Verevon"),
            identified("m1", "user", "hent fraktpriser"),
            identified("m2", "assistant", "her er fraktprisene"),
            identified("m3", "user", "hva er hovedstaden i Danmark?"),
            identified("m4", "assistant", "København."),
        ];
        let expected: Vec<ChatMessage> =
            loaded.iter().map(|entry| entry.message.clone()).collect();

        // `plan_head_summary` returns None for a thread this short, so the bound
        // is 0 and the pin changes nothing about what the model sees.
        let out = hoist_pinned_messages(loaded, &["m2".to_owned()], 0);
        assert_eq!(out, expected);
        assert!(!out.iter().any(|m| m.content.contains("Pinned by the user")));
        // The pinned turn is still exactly where the user wrote it.
        assert_eq!(out[2].role, "assistant");
        assert_eq!(out[2].content, "her er fraktprisene");
    }

    /// A pin ABOVE the compaction boundary is safe where it is, so it stays.
    /// Only the ones about to be summarised away are lifted out.
    #[test]
    fn only_pins_inside_the_compacted_head_are_hoisted() {
        // Head runs to index 35 for this fixture; m37 sits in the kept tail.
        let out = hoist_pinned_messages(
            long_conversation(),
            &["m2".to_owned(), "m37".to_owned()],
            35,
        );
        let hoisted: Vec<&ChatMessage> = out
            .iter()
            .filter(|m| m.content.contains("Pinned by the user"))
            .collect();
        assert_eq!(hoisted.len(), 1, "only the at-risk pin should be hoisted");
        assert!(hoisted[0].content.contains("turn 2"));
        // m37 was never moved: still present once, in the body, unwrapped.
        let in_place: Vec<&ChatMessage> = out
            .iter()
            .filter(|m| m.content == "turn 37")
            .collect();
        assert_eq!(in_place.len(), 1);
    }
}
