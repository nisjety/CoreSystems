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

/// Family hint for protecting pins; not proof of native compaction support.
#[must_use]
pub fn is_anthropic_family_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("claude")
}

/// Keep local recovery available through native-off rollback and provider fallback.
#[must_use]
pub fn should_run_local_compaction(_model: &str) -> bool {
    // Routing can substitute a different provider, and native support/flags
    // belong to inference-core. A model-name guess must never remove recovery.
    true
}

/// Bound tool payloads on every route; compatibility wrapper for existing callers.
pub fn clear_stale_tool_results_unless_native(
    _model: &str,
    messages: &mut [ChatMessage],
    budget: ToolPayloadBudget,
) -> usize {
    clear_stale_tool_results(messages, budget)
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

/// Framing tokens counted per message on top of its content.
const BUDGET_TOKENS_PER_MESSAGE: usize = 4;

/// Fewest history messages token-budget compaction acts on. Low on purpose:
/// two turns carrying one pasted 60 KB table must be able to compact, which a
/// floor of eight plus a 23-message tail never could.
pub const BUDGET_MIN_MESSAGES: usize = 6;

/// Most messages the verbatim tail keeps: the message-count path's tail.
pub const BUDGET_MAX_TAIL_MESSAGES: usize = 23;

/// Fewest messages the verbatim tail keeps: the current turn and the one
/// before it, however large.
pub const BUDGET_MIN_TAIL_MESSAGES: usize = 2;

/// A history longer than this compacts whatever it weighs, so a long thread
/// of one-word replies still stays bounded.
pub const BUDGET_MESSAGE_CEILING: usize = 96;

/// Opening of an attached source inside a user turn: the marker
/// `source_validation::SourceContext::from_messages` reads sources from.
const SOURCE_ATTACHMENT_OPENING: &str = "\n\n--- VEDLEGG: ";

/// Whether a message carries an attached source the source review reads.
///
/// The review checks a draft against each attachment's exact text, found in
/// the user turns of the prompt, so a turn carrying one is never summarised:
/// a summary of a source is not the source.
#[must_use]
pub fn carries_source_attachment(message: &ChatMessage) -> bool {
    message.role == "user" && message.content.contains(SOURCE_ATTACHMENT_OPENING)
}

/// Estimated tokens for one message: UTF-8 bytes over four, rounded up, plus
/// [`BUDGET_TOKENS_PER_MESSAGE`]. The `bytes/4` heuristic the Anthropic
/// adapter already uses; no tokenizer dependency.
#[must_use]
pub fn estimated_message_tokens(message: &ChatMessage) -> usize {
    message.content.len().div_ceil(4) + BUDGET_TOKENS_PER_MESSAGE
}

/// [`plan_head_summary`] by estimated tokens instead of message count.
///
/// Counts the history only, after the leading system messages (pins, a
/// summary, anything else already standing context); the context block and
/// the tools are bounded on their own. Compacts when that history exceeds
/// `budget_tokens` and has at least [`BUDGET_MIN_MESSAGES`] messages, or has
/// more than [`BUDGET_MESSAGE_CEILING`] whatever it weighs. The verbatim tail
/// keeps the newest messages up to [`BUDGET_MAX_TAIL_MESSAGES`] while they fit
/// half the budget, and never fewer than [`BUDGET_MIN_TAIL_MESSAGES`], which
/// leaves the other half as headroom: the turns right after a compaction do
/// not compact again.
///
/// The head stops at the first message it must not take: a system message,
/// or a turn carrying an attached source (see [`carries_source_attachment`]).
/// Everything from there on stays verbatim.
#[must_use]
pub fn plan_head_summary_by_budget(
    messages: &[ChatMessage],
    budget_tokens: usize,
) -> Option<HeadSummary> {
    let messages: Vec<&ChatMessage> = messages.iter().collect();
    plan_by_budget(&messages, budget_tokens)
}

fn plan_by_budget(messages: &[&ChatMessage], budget_tokens: usize) -> Option<HeadSummary> {
    let start = messages
        .iter()
        .position(|message| message.role != "system")
        .unwrap_or(messages.len());
    let history = &messages[start..];
    let estimated: usize = history
        .iter()
        .map(|message| estimated_message_tokens(message))
        .sum();
    let over_budget = estimated > budget_tokens && history.len() >= BUDGET_MIN_MESSAGES;
    if !over_budget && history.len() <= BUDGET_MESSAGE_CEILING {
        return None;
    }
    let tail_budget = budget_tokens / 2;
    let mut tail = 0usize;
    let mut spent = 0usize;
    for message in history.iter().rev() {
        let cost = estimated_message_tokens(message);
        if tail >= BUDGET_MAX_TAIL_MESSAGES
            || (tail >= BUDGET_MIN_TAIL_MESSAGES && spent + cost > tail_budget)
        {
            break;
        }
        spent += cost;
        tail += 1;
    }
    let mut end = messages.len() - tail;
    if let Some(offset) = messages[start..end]
        .iter()
        .position(|message| message.role == "system" || carries_source_attachment(message))
    {
        end = start + offset;
    }
    (end.saturating_sub(start) >= 2).then_some(HeadSummary { start, end })
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
        return format!("{base} {SUMMARY_STABILITY_CLAUSE}\n\nEXCERPT:\n{transcript}");
    }
    format!(
        "{base} {SUMMARY_STABILITY_CLAUSE}\n\nThe user has previously established the topics \
         below. If — and only if — the excerpt touches any of them, preserve what it says about \
         them verbatim. These are a hint about what matters, NOT facts to add: never state \
         anything from this list that the excerpt itself does not say.\n\nPREVIOUSLY \
         ESTABLISHED:\n{directive}\n\nEXCERPT:\n{transcript}"
    )
}

/// Closes the summary instruction on every route (cache audit §4.3).
///
/// A summary is sent again on later turns: as the head of the prompt until
/// the thread compacts again, and, with token-budget compaction, byte for
/// byte from the persisted copy. Anything in it that would differ on an
/// identical rerun — today's date, a message count, a remark about the
/// summarising itself — makes that prefix unrepeatable and is not in the
/// excerpt anyway.
pub const SUMMARY_STABILITY_CLAUSE: &str = "These notes become a fixed prefix that later turns \
     reuse unchanged, so copy only what the excerpt states and never add the current date or \
     time, message counts or remarks about the summary itself.";

/// Sampling temperature for a summary call, for the same reason as
/// [`SUMMARY_STABILITY_CLAUSE`]. It reaches only an adapter that forwards a
/// temperature: `OpenAI` non-reasoning models. Reasoning models, Claude and
/// the subscription broker send none.
pub const SUMMARY_TEMPERATURE: f32 = 0.0;

/// The single retained message a successful summary becomes.
#[must_use]
pub fn summary_message(summary: &str) -> ChatMessage {
    ChatMessage {
        compaction_summary: String::new(),
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
            compaction_summary: String::new(),
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
    hoist_pinned_identified(loaded, pinned_ids, protect_below)
        .messages
        .into_iter()
        .map(|entry| entry.message)
        .collect()
}

/// [`hoist_pinned_messages`] with the durable ids kept, and the pins it moved.
#[derive(Clone, Debug)]
pub struct HoistedPins {
    /// The messages in order. A hoisted pin is new system context and has no
    /// durable id of its own; every other entry keeps its id.
    pub messages: Vec<IdentifiedMessage>,
    /// Ids of the pins moved into the leading system block, in pin order.
    pub hoisted_ids: Vec<String>,
}

/// [`hoist_pinned_messages`], keeping ids so a caller can still name the
/// messages it compacts.
#[must_use]
pub fn hoist_pinned_identified(
    loaded: Vec<IdentifiedMessage>,
    pinned_ids: &[String],
    protect_below: usize,
) -> HoistedPins {
    if pinned_ids.is_empty() || protect_below == 0 {
        return HoistedPins {
            messages: loaded,
            hoisted_ids: Vec::new(),
        };
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

    let mut hoisted: Vec<IdentifiedMessage> = Vec::new();
    let mut hoisted_ids: Vec<String> = Vec::new();
    let mut body: Vec<IdentifiedMessage> = Vec::with_capacity(loaded.len());
    let mut pinned_by_id: Vec<(usize, IdentifiedMessage)> = Vec::new();

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
            Some(rank) => pinned_by_id.push((rank, entry)),
            None => body.push(entry),
        }
    }

    pinned_by_id.sort_by_key(|(rank, _)| *rank);

    let mut budget = MAX_PINNED_CHARS;
    for (_, entry) in pinned_by_id {
        if budget == 0 {
            // Out of budget: the message stays in the body where it was, rather
            // than vanishing. It is then droppable like any other turn, which is
            // honest — it was not protected.
            body.insert(0, entry);
            continue;
        }
        let IdentifiedMessage { id, message } = entry;
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
        hoisted.push(IdentifiedMessage {
            id: String::new(),
            message: ChatMessage {
                compaction_summary: String::new(),
                role: "system".to_owned(),
                content: block,
                name: String::new(),
            },
        });
        hoisted_ids.push(id);
    }

    hoisted.extend(body);
    HoistedPins {
        messages: hoisted,
        hoisted_ids,
    }
}

/// A tier-2 summary persisted with the assistant message of the turn that
/// wrote it (token-budget mode), so later turns send the same bytes instead
/// of summarising the same head again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoredHeadSummary {
    /// The summary text, trimmed: exactly what [`summary_message`] wraps.
    pub summary: String,
    /// Durable id of the last message the summary covers. It covers every
    /// history message from the first non-system one through this one.
    pub through_id: String,
    /// Pins hoisted out of that stretch when it was summarised. The summary
    /// does not contain them, so it is reused only while they are all still
    /// pinned and hoisted again.
    pub pinned_ids: Vec<String>,
}

/// What token-budget compaction sends, before any new summary is written.
#[derive(Clone, Debug)]
pub struct BudgetCompaction {
    /// The thread with a reusable persisted summary spliced in and pins
    /// hoisted. Final unless [`Self::head`] is set.
    pub messages: Vec<ChatMessage>,
    /// A head still to summarise, in `messages` coordinates. When a summary
    /// was reused, the head starts at it, so the new summary carries the old
    /// one forward.
    pub head: Option<HeadSummary>,
    /// Whether a persisted summary was reused.
    pub reused: bool,
    /// Durable id of the head's last message.
    through_id: Option<String>,
    /// Pins hoisted this turn.
    hoisted_ids: Vec<String>,
}

impl BudgetCompaction {
    /// The record to persist once `summary` has replaced [`Self::head`].
    /// `None` without a head, for an empty summary, or when the head's last
    /// message has no durable id to name.
    #[must_use]
    pub fn record(&self, summary: &str) -> Option<StoredHeadSummary> {
        let summary = summary.trim();
        let through_id = self.through_id.as_deref()?.trim();
        if self.head.is_none() || summary.is_empty() || through_id.is_empty() {
            return None;
        }
        Some(StoredHeadSummary {
            summary: summary.to_owned(),
            through_id: through_id.to_owned(),
            pinned_ids: self.hoisted_ids.clone(),
        })
    }
}

/// Plan token-budget compaction for a loaded thread (ending with the current
/// turn). Pure; the summarizer call stays at the caller's IO edge.
///
/// A `stored` summary is reused when the message it covers through is still
/// in the thread before the current turn, nothing it covers carries an
/// attached source, and every pin it left out is still pinned. Reused, it
/// replaces the stretch it covers with exactly the message it produced before
/// ([`summary_message`]), and only the history after it counts against the
/// budget. Covered messages that are pinned stay, for the hoist to lift out as
/// it did when the summary was written.
#[must_use]
pub fn prepare_budget_compaction(
    loaded: Vec<IdentifiedMessage>,
    stored: Option<&StoredHeadSummary>,
    pinned_ids: &[String],
    budget_tokens: usize,
) -> BudgetCompaction {
    let start = loaded
        .iter()
        .position(|entry| entry.message.role != "system")
        .unwrap_or(loaded.len());
    let reusable = stored.and_then(|stored| {
        reusable_through(&loaded, start, stored, pinned_ids).map(|through| (stored, through))
    });
    let (view, summary_at) = match reusable {
        Some((stored, through)) => {
            let is_pinned = |entry: &IdentifiedMessage| {
                entry.message.role != "system"
                    && !entry.id.trim().is_empty()
                    && pinned_ids.contains(&entry.id)
            };
            let mut view = Vec::with_capacity(loaded.len());
            let mut summary_at = None;
            for (index, entry) in loaded.into_iter().enumerate() {
                if (start..=through).contains(&index) {
                    if is_pinned(&entry) {
                        view.push(entry);
                    }
                    continue;
                }
                if index == through + 1 {
                    summary_at = Some(view.len());
                    view.push(IdentifiedMessage {
                        id: String::new(),
                        message: summary_message(&stored.summary),
                    });
                }
                view.push(entry);
            }
            (view, summary_at)
        }
        None => (loaded, None),
    };

    // How far the shed region reaches, measured before hoisting as the
    // message-count path measures it; a reused summary's pins are inside it.
    let refs: Vec<&ChatMessage> = view.iter().map(|entry| &entry.message).collect();
    let planned_end = plan_by_budget(&refs, budget_tokens).map_or(0, |head| head.end);
    let protect_below = summary_at.map_or(planned_end, |at| planned_end.max(at + 1));
    let HoistedPins {
        messages: hoisted,
        hoisted_ids,
    } = hoist_pinned_identified(view, pinned_ids, protect_below);

    let summary_index = reusable.and_then(|(stored, _)| {
        let content = summary_message(&stored.summary).content;
        hoisted.iter().position(|entry| {
            entry.id.is_empty()
                && entry.message.role == "system"
                && entry.message.content == content
        })
    });
    let refs: Vec<&ChatMessage> = hoisted.iter().map(|entry| &entry.message).collect();
    let head = plan_by_budget(&refs, budget_tokens).map(|mut head| {
        if summary_index.is_some_and(|at| at + 1 == head.start) {
            head.start -= 1;
        }
        head
    });
    let through_id = head
        .and_then(|head| hoisted.get(head.end.checked_sub(1)?))
        .map(|entry| entry.id.clone());
    BudgetCompaction {
        messages: hoisted.into_iter().map(|entry| entry.message).collect(),
        head,
        reused: summary_at.is_some(),
        through_id,
        hoisted_ids,
    }
}

/// Index of the message `stored` covers through, when it may be reused.
fn reusable_through(
    loaded: &[IdentifiedMessage],
    start: usize,
    stored: &StoredHeadSummary,
    pinned_ids: &[String],
) -> Option<usize> {
    let through_id = stored.through_id.trim();
    if through_id.is_empty() || stored.summary.trim().is_empty() {
        return None;
    }
    let through = loaded.iter().position(|entry| entry.id == through_id)?;
    // The summary stands in for whole turns ahead of the current one.
    if through < start || through + 1 >= loaded.len() {
        return None;
    }
    let covered = &loaded[start..=through];
    if covered
        .iter()
        .any(|entry| entry.message.role == "system" || carries_source_attachment(&entry.message))
    {
        return None;
    }
    stored
        .pinned_ids
        .iter()
        .all(|id| pinned_ids.contains(id))
        .then_some(through)
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
                compaction_summary: String::new(),
                role: "system".to_owned(),
                content: DROPPED_HISTORY_NOTICE.to_owned(),
                name: String::new(),
            },
        );
    }
    true
}

/// [`drop_oldest_group`] for a prompt whose per-turn system blocks sit
/// between the prior history and the current turn
/// (`prompt_context::PromptOrder::StablePrefix`).
///
/// Drops the oldest `group` non-system messages outside the protected tail
/// and leaves every system message where it is, so grounding and memory are
/// not shed along with the history around them. Same notice, and the same
/// `false` once nothing droppable is left.
pub fn drop_oldest_history_group(
    messages: &mut Vec<ChatMessage>,
    group: usize,
    keep_tail: usize,
) -> bool {
    let start = messages
        .iter()
        .position(|message| message.role != "system")
        .unwrap_or(messages.len());
    let limit = messages.len().saturating_sub(keep_tail);
    let droppable: Vec<usize> = (start..limit)
        .filter(|index| messages[*index].role != "system")
        .take(group.max(1))
        .collect();
    if droppable.is_empty() {
        return false;
    }
    for index in droppable.into_iter().rev() {
        messages.remove(index);
    }
    if !messages
        .iter()
        .any(|message| message.content == DROPPED_HISTORY_NOTICE)
    {
        messages.insert(
            start,
            ChatMessage {
                compaction_summary: String::new(),
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
/// spellings together. `subscription_failure` reads the same key for a
/// subscription refusal's reason.
pub(crate) const PROVIDER_ERROR_KIND_TRAILER: &str = "x-mp-provider-error";

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
            compaction_summary: String::new(),
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

    // --- native-compaction migration: the Anthropic-family dispatch --------

    #[test]
    fn anthropic_family_models_are_recognized_case_and_whitespace_insensitively() {
        for model in [
            "claude-sonnet-4-6",
            "Claude-Opus-4-8",
            "  claude-haiku-4-5  ",
            "CLAUDE-3-5-SONNET-20241022",
        ] {
            assert!(
                is_anthropic_family_model(model),
                "{model:?} should be recognized as Anthropic-family"
            );
        }
    }

    #[test]
    fn non_anthropic_and_unspecified_models_are_not_anthropic_family() {
        for model in ["gpt-4o-mini", "model-router", "gemini-1.5-pro", "", "   "] {
            assert!(
                !is_anthropic_family_model(model),
                "{model:?} must not be treated as Anthropic-family"
            );
        }
    }

    #[test]
    fn local_recovery_does_not_depend_on_a_provider_name_guess() {
        assert!(should_run_local_compaction("claude-sonnet-4-6"));
        assert!(should_run_local_compaction("claude-haiku-4-5"));
        assert!(should_run_local_compaction("gpt-4o-mini"));
        // An empty/unspecified model cannot be trusted to resolve to
        // Anthropic, so the SAFE default is to keep running the existing
        // hand-rolled path rather than silently skip it.
        assert!(should_run_local_compaction(""));
    }

    /// **Migration test 1:** a turn resolved to the Anthropic family does NOT
    /// also run the hand-rolled tier-1 clearing — inference-core's native
    /// `clear_tool_uses_20250919` edit owns that job for this provider.
    #[test]
    fn anthropic_turn_keeps_local_tool_payload_recovery() {
        let budget = ToolPayloadBudget {
            max_chars: 10,
            keep_recent: 0,
        };
        let mut messages = vec![tool_result(500), tool_result(500), tool_result(500)];

        let cleared =
            clear_stale_tool_results_unless_native("claude-sonnet-4-6", &mut messages, budget);

        assert_eq!(cleared, 3);
        assert!(messages.iter().all(|message| message.content.contains(CLEARED_TOOL_RESULT_NOTICE)));
    }

    /// **Migration test 2:** a turn resolved to a non-Anthropic model still
    /// runs the existing hand-rolled clearing, unchanged — proven by
    /// asserting `clear_stale_tool_results_unless_native` behaves byte-for-
    /// byte like the pre-migration `clear_stale_tool_results` on the exact
    /// fixture `over_budget_clears_oldest_and_keeps_the_newest_results` uses.
    #[test]
    fn a_non_anthropic_turn_still_runs_the_hand_rolled_clearing_unchanged() {
        let budget = ToolPayloadBudget {
            max_chars: 2_500,
            keep_recent: 2,
        };
        let mut via_wrapper = vec![
            message("system", "grounding"),
            message("user", "question"),
            tool_result(1_000),
            tool_result(1_000),
            tool_result(1_000),
            tool_result(1_000),
        ];
        let mut via_direct = via_wrapper.clone();

        let cleared_by_wrapper =
            clear_stale_tool_results_unless_native("gpt-4o-mini", &mut via_wrapper, budget);
        let cleared_directly = clear_stale_tool_results(&mut via_direct, budget);

        assert_eq!(cleared_by_wrapper, cleared_directly);
        assert_eq!(via_wrapper, via_direct);
        assert_eq!(cleared_by_wrapper, 2);
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

    // --- cache stability of the summary (all routes) ---------------------

    #[test]
    fn both_summary_prompt_variants_carry_the_stability_clause() {
        let transcript = "user: bruk NOK\nassistant: notert\n";
        for prompt in [
            summary_prompt(transcript),
            summary_prompt_with_memory(transcript, "- prefers invoices in NOK\n"),
        ] {
            assert!(prompt.contains(SUMMARY_STABILITY_CLAUSE), "{prompt}");
            assert!(
                prompt.find(SUMMARY_STABILITY_CLAUSE) < prompt.find("EXCERPT:"),
                "the clause is part of the instruction, not the excerpt"
            );
        }
        assert!(SUMMARY_STABILITY_CLAUSE.contains("fixed prefix"));
        assert!(SUMMARY_STABILITY_CLAUSE.contains("never add the current date or time"));
        assert!(SUMMARY_STABILITY_CLAUSE.contains("message counts"));
        assert!(SUMMARY_TEMPERATURE.abs() < f32::EPSILON);
    }

    // --- token-budget compaction -----------------------------------------

    const BUDGET: usize = 12_000;

    fn sized(role: &str, label: &str, bytes: usize) -> ChatMessage {
        message(role, &format!("{label} {}", "x".repeat(bytes)))
    }

    fn with_attachment(instruction: &str, name: &str, body: &str) -> String {
        format!(
            "{instruction}\n\n--- VEDLEGG: {name} ---\n{body}\n--- SLUTT PÅ VEDLEGG: {name} ---"
        )
    }

    #[test]
    fn thirty_one_word_messages_are_not_compacted_by_budget() {
        let messages: Vec<ChatMessage> = (0..30)
            .map(|i| message(if i % 2 == 0 { "user" } else { "assistant" }, "ok"))
            .collect();
        assert_eq!(plan_head_summary_by_budget(&messages, BUDGET), None);
        // The message-count path would have compacted this thread.
        assert!(plan_head_summary(&messages, 24, 23).is_some());
    }

    #[test]
    fn a_pasted_table_compacts_a_short_thread_and_keeps_the_current_turn() {
        let mut messages = vec![
            message("system", "identity"),
            message("user", "hei"),
            message("assistant", "hei"),
            sized("user", "tabell:", 60_000),
            message("assistant", "notert"),
        ];
        for turn in ["q1", "a1", "q2", "a2"] {
            messages.push(message(
                if turn.starts_with('q') {
                    "user"
                } else {
                    "assistant"
                },
                turn,
            ));
        }
        messages.push(message("user", "current question"));
        assert_eq!(messages.len() - 1, 9, "nine history messages");

        let head =
            plan_head_summary_by_budget(&messages, BUDGET).expect("the table is over budget");
        assert_eq!(
            head,
            HeadSummary { start: 1, end: 4 },
            "the head ends with the table"
        );
        let compacted = apply_head_summary(messages, head, Some("user pasted a stock table"));
        assert_eq!(compacted[0].content, "identity");
        assert!(compacted[1].content.starts_with(SUMMARY_PREFIX));
        assert!(!compacted.iter().any(|m| m.content.starts_with("tabell:")));
        assert_eq!(
            compacted.last().map(|m| m.content.as_str()),
            Some("current question")
        );
    }

    #[test]
    fn leading_system_messages_are_never_compacted_by_budget() {
        let mut messages = vec![message("system", "identity"), message("system", "pin")];
        messages.extend((0..40).map(|i| sized("user", &format!("turn {i}"), 2_000)));
        let head = plan_head_summary_by_budget(&messages, BUDGET).expect("40 x 500 tokens");
        assert_eq!(head.start, 2);
        let compacted = apply_head_summary(messages, head, Some("summary"));
        assert_eq!(compacted[0].content, "identity");
        assert_eq!(compacted[1].content, "pin");
    }

    #[test]
    fn a_fresh_budget_compaction_does_not_trigger_again_on_the_next_turn() {
        let mut messages = vec![message("system", "identity")];
        messages.extend((0..40).map(|i| {
            sized(
                if i % 2 == 0 { "user" } else { "assistant" },
                &format!("turn {i}"),
                1_200,
            )
        }));
        let head = plan_head_summary_by_budget(&messages, BUDGET).expect("over budget");
        let mut compacted = apply_head_summary(messages, head, Some("summary"));
        let history: usize = compacted[2..].iter().map(estimated_message_tokens).sum();
        assert!(
            history <= BUDGET / 2,
            "the tail fits half the budget: {history}"
        );
        compacted.push(message("assistant", "answer"));
        compacted.push(message("user", "next question"));
        assert_eq!(plan_head_summary_by_budget(&compacted, BUDGET), None);
    }

    #[test]
    fn the_verbatim_tail_is_bounded_both_ways() {
        // Cheap messages: the tail stops at 23.
        let cheap: Vec<ChatMessage> = (0..97).map(|i| message("user", &i.to_string())).collect();
        let head = plan_head_summary_by_budget(&cheap, BUDGET).expect("past the ceiling");
        assert_eq!(cheap.len() - head.end, BUDGET_MAX_TAIL_MESSAGES);
        // Huge final turns: the tail still keeps two.
        let mut heavy: Vec<ChatMessage> = (0..6).map(|i| message("user", &i.to_string())).collect();
        heavy.push(sized("assistant", "report", 30_000));
        heavy.push(sized("user", "follow-up with a pasted log", 30_000));
        let head = plan_head_summary_by_budget(&heavy, BUDGET).expect("over budget");
        assert_eq!(heavy.len() - head.end, BUDGET_MIN_TAIL_MESSAGES);
    }

    #[test]
    fn the_message_ceiling_compacts_a_long_cheap_thread() {
        let at_ceiling: Vec<ChatMessage> = (0..BUDGET_MESSAGE_CEILING)
            .map(|i| message("user", &i.to_string()))
            .collect();
        assert_eq!(plan_head_summary_by_budget(&at_ceiling, BUDGET), None);
        let mut past = at_ceiling;
        past.push(message("user", "one more"));
        assert!(plan_head_summary_by_budget(&past, BUDGET).is_some());
    }

    #[test]
    fn the_attachment_marker_is_what_the_source_review_reads() {
        let scope = message(
            "system",
            crate::source_validation::CONVERSATION_SCOPE_NOTICE,
        );
        let attached = message(
            "user",
            &with_attachment("Lag et svar.", "kilde.md", "9 lamper"),
        );
        assert!(carries_source_attachment(&attached));
        assert!(crate::source_validation::SourceContext::from_messages(&[
            scope.clone(),
            attached.clone()
        ])
        .is_some());
        let plain = message("user", "Lag et svar.");
        assert!(!carries_source_attachment(&plain));
        assert!(
            crate::source_validation::SourceContext::from_messages(&[scope.clone(), plain])
                .is_none()
        );
        // The review reads user turns only, and so does the protection.
        let quoted = message("assistant", &attached.content);
        assert!(!carries_source_attachment(&quoted));
        assert!(crate::source_validation::SourceContext::from_messages(&[scope, quoted]).is_none());
    }

    #[test]
    fn source_bound_turns_keep_their_sources() {
        let scope = message(
            "system",
            crate::source_validation::CONVERSATION_SCOPE_NOTICE,
        );
        let attached = message(
            "user",
            &with_attachment(
                "Svar kunden.",
                "kildepakke.md",
                "9 lamper er plukket og pakket",
            ),
        );

        // Sources in the first turn: nothing ahead of them to compact.
        let mut first = vec![scope.clone(), attached.clone()];
        first.extend((0..40).map(|i| sized("assistant", &format!("draft {i}"), 1_500)));
        first.push(message("user", "current"));
        assert_eq!(plan_head_summary_by_budget(&first, BUDGET), None);

        // Sources mid-thread: the head stops right before them.
        let mut messages = vec![scope];
        messages.extend((0..12).map(|i| sized("user", &format!("chatter {i}"), 1_500)));
        let attached_at = messages.len();
        messages.push(attached);
        messages.extend((0..40).map(|i| sized("assistant", &format!("draft {i}"), 1_500)));
        messages.push(message("user", "current"));
        let before = crate::source_validation::SourceContext::from_messages(&messages)
            .expect("the thread is source-bound")
            .hash();

        let head = plan_head_summary_by_budget(&messages, BUDGET).expect("over budget");
        assert_eq!(
            head,
            HeadSummary {
                start: 1,
                end: attached_at
            }
        );
        let compacted = apply_head_summary(messages, head, Some("chatter summary"));
        let after = crate::source_validation::SourceContext::from_messages(&compacted)
            .expect("the sources survive compaction")
            .hash();
        assert_eq!(after, before);
    }

    fn long_identified(turns: usize, bytes: usize) -> Vec<IdentifiedMessage> {
        let mut out = vec![identified("", "system", "You are Verevon")];
        for index in 0..turns {
            out.push(identified(
                &format!("m{index}"),
                if index % 2 == 0 { "user" } else { "assistant" },
                &format!("turn {index} {}", "x".repeat(bytes)),
            ));
        }
        out
    }

    fn plain(entries: &[IdentifiedMessage]) -> Vec<ChatMessage> {
        entries.iter().map(|entry| entry.message.clone()).collect()
    }

    /// Turn N writes a summary; turn N+1 sends it again byte for byte, with no
    /// summary call, and its prompt opens with everything turn N sent.
    #[test]
    fn a_stored_summary_is_reused_byte_for_byte_without_a_new_summary_call() {
        let loaded = long_identified(41, 1_200);
        let turn_n = prepare_budget_compaction(loaded.clone(), None, &[], BUDGET);
        assert!(!turn_n.reused);
        let head = turn_n.head.expect("over budget");
        let record = turn_n.record("  K3; 9 packed; NOK  ").expect("a record");
        assert_eq!(record.summary, "K3; 9 packed; NOK");
        assert_eq!(
            record.through_id,
            format!("m{}", head.end - 2),
            "ids follow the leading system message"
        );
        let sent_n = apply_head_summary(turn_n.messages, head, Some("  K3; 9 packed; NOK  "));

        let mut next = loaded;
        next.push(identified("m41", "assistant", "answer"));
        next.push(identified("m42", "user", "next question"));
        let turn_next = prepare_budget_compaction(next, Some(&record), &[], BUDGET);
        assert!(turn_next.reused);
        assert_eq!(
            turn_next.head, None,
            "nothing to summarise, so no summary call"
        );
        assert_eq!(&turn_next.messages[..sent_n.len()], sent_n.as_slice());
        assert_eq!(
            turn_next.messages.last().map(|m| m.content.as_str()),
            Some("next question")
        );
    }

    #[test]
    fn a_reused_summary_is_carried_into_the_next_one_only_past_the_budget() {
        let loaded = long_identified(41, 1_200);
        let turn_n = prepare_budget_compaction(loaded.clone(), None, &[], BUDGET);
        let record = turn_n.record("old summary").expect("a record");

        let mut next = loaded;
        for index in 41..71 {
            next.push(identified(
                &format!("m{index}"),
                if index % 2 == 0 { "user" } else { "assistant" },
                &format!("turn {index} {}", "y".repeat(1_200)),
            ));
        }
        let plan = prepare_budget_compaction(next, Some(&record), &[], BUDGET);
        assert!(plan.reused);
        let head = plan
            .head
            .expect("the uncovered history is over budget again");
        assert!(plan.messages[head.start]
            .content
            .starts_with(SUMMARY_PREFIX));
        assert!(render_head_transcript(&plan.messages, head).contains("old summary"));
        let renewed = plan.record("new summary").expect("a record");
        assert_ne!(renewed.through_id, record.through_id);
        let compacted = apply_head_summary(plan.messages, head, Some("new summary"));
        assert_eq!(
            compacted
                .iter()
                .filter(|m| m.content.starts_with(SUMMARY_PREFIX))
                .count(),
            1,
            "the new summary replaces the old one"
        );
    }

    #[test]
    fn a_stale_summary_is_not_reused() {
        let loaded = long_identified(41, 1_200);
        let fresh =
            contents(&prepare_budget_compaction(loaded.clone(), None, &[], BUDGET).messages);
        let stale = |stored: StoredHeadSummary, loaded: Vec<IdentifiedMessage>, pins: &[String]| {
            let plan = prepare_budget_compaction(loaded, Some(&stored), pins, BUDGET);
            assert!(!plan.reused, "{stored:?}");
            plan
        };
        let stored = |through: &str, pins: &[&str]| StoredHeadSummary {
            summary: "old".to_owned(),
            through_id: through.to_owned(),
            pinned_ids: pins.iter().map(|id| (*id).to_owned()).collect(),
        };
        // The covered message is gone.
        assert_eq!(
            contents(&stale(stored("gone", &[]), loaded.clone(), &[]).messages),
            fresh
        );
        // It names the current turn.
        stale(stored("m40", &[]), loaded.clone(), &[]);
        // A pin it left out is no longer pinned, so the summary lacks it.
        stale(stored("m10", &["m3"]), loaded.clone(), &[]);
        // It covers an attached source.
        let mut attached = loaded;
        attached[1].message.content = with_attachment("Svar.", "kilde.md", "fakta");
        stale(stored("m10", &[]), attached, &[]);
    }

    fn contents(messages: &[ChatMessage]) -> Vec<String> {
        messages.iter().map(|m| m.content.clone()).collect()
    }

    #[test]
    fn pins_survive_budget_compaction_and_its_reuse() {
        let loaded = long_identified(41, 1_200);
        let pins = vec!["m3".to_owned()];
        let turn_n = prepare_budget_compaction(loaded.clone(), None, &pins, BUDGET);
        let head = turn_n.head.expect("over budget");
        assert!(turn_n.messages[0].content.contains("Pinned by the user"));
        assert!(turn_n.messages[0].content.contains("turn 3 "));
        let record = turn_n.record("summary").expect("a record");
        assert_eq!(record.pinned_ids, pins);
        let sent_n = apply_head_summary(turn_n.messages, head, Some("summary"));

        let mut next = loaded;
        next.push(identified("m41", "assistant", "answer"));
        next.push(identified("m42", "user", "next"));
        let reused = prepare_budget_compaction(next, Some(&record), &pins, BUDGET);
        assert!(reused.reused);
        assert_eq!(&reused.messages[..sent_n.len()], sent_n.as_slice());
        assert_eq!(
            reused
                .messages
                .iter()
                .filter(|m| m.content.contains("turn 3 "))
                .count(),
            1,
            "hoisted again, once"
        );
    }

    #[test]
    fn the_identified_hoist_matches_the_plain_one_and_names_what_it_moved() {
        let pins = ["m9".to_owned(), "m3".to_owned()];
        let hoisted = hoist_pinned_identified(long_conversation(), &pins, 41);
        assert_eq!(hoisted.hoisted_ids, ["m9", "m3"]);
        assert_eq!(
            plain(&hoisted.messages),
            hoist_pinned_messages(long_conversation(), &pins, 41)
        );
        assert!(hoisted.messages[..2]
            .iter()
            .all(|entry| entry.id.is_empty()));
        assert!(hoisted.messages[2..]
            .iter()
            .all(|entry| entry.message.role == "system" || !entry.id.is_empty()));
    }

    #[test]
    fn dropping_history_keeps_the_per_turn_system_blocks() {
        let mut messages = vec![
            message("system", "standing"),
            message("user", "old question"),
            message("assistant", "old answer"),
            message("system", "grounding"),
            message("system", "memory"),
            message("user", "current"),
        ];
        messages.extend((0..4).map(|_| tool_result(100)));
        assert!(drop_oldest_history_group(&mut messages, 6, 4));
        while drop_oldest_history_group(&mut messages, 6, 4) {}
        for kept in ["standing", "grounding", "memory"] {
            assert!(
                messages.iter().any(|m| m.content == kept),
                "{kept} was shed"
            );
        }
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.content == DROPPED_HISTORY_NOTICE)
                .count(),
            1
        );
        assert!(!messages.iter().any(|m| m.content == "old question"));
        assert!(
            messages.len() >= 4 + 3,
            "the tail and the system blocks remain"
        );
        // The plain drop takes the per-turn blocks with the history.
        let mut plain_drop = vec![
            message("system", "standing"),
            message("user", "old question"),
            message("system", "grounding"),
            message("user", "current"),
        ];
        assert!(drop_oldest_group(&mut plain_drop, 6, 1));
        assert!(!plain_drop.iter().any(|m| m.content == "grounding"));
    }
}
