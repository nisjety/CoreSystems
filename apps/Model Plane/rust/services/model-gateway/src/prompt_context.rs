//! The chat prompt's system blocks as named slots, the order they are sent
//! in, and the prefix-cache options that only pay off on `OpenAI`-API routes.
//!
//! `OpenAI` and Azure `OpenAI` chat completions cache the longest repeated
//! prefix of a prompt automatically: the tools first, then the messages in
//! order. The chat path has always put per-turn blocks (artifact inventory,
//! grounding, session-core's context block, memory recall, skills) ahead of
//! the standing ones and ahead of the whole conversation, so almost nothing
//! past the first kilobyte repeats between turns.
//! [`PromptOrder::StablePrefix`] moves those blocks to just before the
//! current user turn, so the standing blocks and the prior conversation form
//! one prefix that later turns repeat byte for byte.
//!
//! Every option here is off unless its flag is set, and applies only to a turn
//! pinned to an `OpenAI`-API chat-completions provider ([`CacheRoute`]).
//! Anthropic joins every system message into one block before caching, so a
//! reorder changes nothing there, and the `ChatGPT` subscription broker starts
//! a fresh Codex thread per call and caches nothing between calls. Both routes
//! keep exactly the prompt they had.

use mp_contracts::model_plane::v1::{ChatMessage, ToolDefinition};

/// Operator switch for [`PromptOrder::StablePrefix`], the conditional tools
/// moved to the end of the tool list, and the final answer resending the tool
/// rounds' tools. Off unless set to `1`, `on`, `true`, `yes` or `enabled`.
pub const STABLE_PROMPT_ORDER_ENV: &str = "MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER";

/// Operator switch for token-budget compaction: the budget in estimated
/// tokens. Unset or 0 keeps the 24/23 message-count compaction.
pub const COMPACTION_TOKEN_BUDGET_ENV: &str = "MODEL_GATEWAY_COMPACTION_TOKEN_BUDGET";

/// Operator switch for leaving out of session-core's context block the
/// history messages the prompt already carries. Off unless set like
/// [`STABLE_PROMPT_ORDER_ENV`].
pub const DEDUP_CONTEXT_HISTORY_ENV: &str = "MODEL_GATEWAY_DEDUP_CONTEXT_HISTORY";

/// Bounds on a set compaction budget. Below the floor nearly every turn
/// would compact; above the ceiling the budget never fires before a
/// provider's own limit does.
const MIN_COMPACTION_TOKEN_BUDGET: usize = 1_000;
const MAX_COMPACTION_TOKEN_BUDGET: usize = 200_000;

/// Provider hints that can only resolve to an `OpenAI`-API chat-completions
/// provider in inference-core: the direct `openai` provider, and the Azure
/// `OpenAI` provider, which also answers to `openai` and `azure`. Cohere,
/// Claude and the subscription broker answer to none of them.
const OPENAI_CHAT_COMPLETIONS_HINTS: [&str; 3] = ["openai", "azure-openai", "azure"];

/// Whether a turn's route is one the prefix-cache options are for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheRoute {
    /// Pinned to an `OpenAI`-API or Azure `OpenAI` chat-completions provider.
    OpenAiChatCompletions,
    /// Claude, the `ChatGPT` subscription, or a route the gateway cannot know
    /// before the first call.
    Other,
}

impl CacheRoute {
    /// The route a turn is pinned to before its first provider call.
    ///
    /// The gateway learns which provider served a call only from the reply
    /// (`provider_used`), and the prompt is assembled before the first call,
    /// so this reads what the turn pins: an `OpenAI` provider hint, no
    /// subscription, and a model that is not Claude. An unhinted turn is
    /// [`CacheRoute::Other`]: inference-core walks `INFERENCE_PROVIDER_ORDER`
    /// (Anthropic first by default), and where it lands is not known here.
    ///
    /// Hints are compared the way inference-core's `hint_matches` compares
    /// them: trimmed, lowercase, `_` read as `-`.
    #[must_use]
    pub fn of_turn(provider_hint: &str, subscription_connection_id: &str, model: &str) -> Self {
        let hint = provider_hint.trim().to_ascii_lowercase().replace('_', "-");
        let subscription =
            crate::pricing::CostBasis::of_route(provider_hint, subscription_connection_id)
                == crate::pricing::CostBasis::Subscription;
        if subscription
            || crate::compaction::is_anthropic_family_model(model)
            || !OPENAI_CHAT_COMPLETIONS_HINTS.contains(&hint.as_str())
        {
            Self::Other
        } else {
            Self::OpenAiChatCompletions
        }
    }
}

/// The prefix-cache options in force for one turn. The default is every
/// option off, which is the prompt the chat path has always sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheOptions {
    /// Send [`PromptOrder::StablePrefix`], put the conditional tools last,
    /// and let the final answer resend the tool rounds' tools.
    pub stable_prompt_order: bool,
    /// Compact by estimated tokens instead of by message count.
    pub compaction_token_budget: Option<usize>,
    /// Leave out of session-core's context block the history the prompt
    /// already carries as messages.
    pub dedup_context_history: bool,
}

impl CacheOptions {
    /// The operator's settings, from the three flag values.
    #[must_use]
    pub fn parse(
        stable_prompt_order: Option<&str>,
        compaction_token_budget: Option<&str>,
        dedup_context_history: Option<&str>,
    ) -> Self {
        Self {
            stable_prompt_order: enabled(stable_prompt_order),
            compaction_token_budget: token_budget(compaction_token_budget),
            dedup_context_history: enabled(dedup_context_history),
        }
    }

    /// The operator's settings, read once like `tool_loop::max_tool_rounds`.
    #[must_use]
    pub fn from_env() -> Self {
        static OPTIONS: std::sync::OnceLock<CacheOptions> = std::sync::OnceLock::new();
        *OPTIONS.get_or_init(|| {
            let read = |name: &str| std::env::var(name).ok();
            Self::parse(
                read(STABLE_PROMPT_ORDER_ENV).as_deref(),
                read(COMPACTION_TOKEN_BUDGET_ENV).as_deref(),
                read(DEDUP_CONTEXT_HISTORY_ENV).as_deref(),
            )
        })
    }

    /// These settings as they apply on `route`: unchanged on an `OpenAI`
    /// chat-completions route, all off anywhere else.
    #[must_use]
    pub fn for_route(self, route: CacheRoute) -> Self {
        match route {
            CacheRoute::OpenAiChatCompletions => self,
            CacheRoute::Other => Self::default(),
        }
    }

    /// The options for a turn with this route and model.
    #[must_use]
    pub fn for_turn(provider_hint: &str, subscription_connection_id: &str, model: &str) -> Self {
        Self::from_env().for_route(CacheRoute::of_turn(
            provider_hint,
            subscription_connection_id,
            model,
        ))
    }

    /// The order this turn's prompt is sent in.
    #[must_use]
    pub fn prompt_order(self) -> PromptOrder {
        if self.stable_prompt_order {
            PromptOrder::StablePrefix
        } else {
            PromptOrder::Legacy
        }
    }
}

fn enabled(value: Option<&str>) -> bool {
    matches!(
        value
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "on" | "true" | "yes" | "enabled"
    )
}

fn token_budget(value: Option<&str>) -> Option<usize> {
    value
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|budget| *budget > 0)
        .map(|budget| budget.clamp(MIN_COMPACTION_TOKEN_BUDGET, MAX_COMPACTION_TOKEN_BUDGET))
}

/// Where the per-turn blocks go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptOrder {
    /// The order the chat path has always sent: conversation scope, artifact
    /// inventory, authored instructions, identity, calendar, date notice,
    /// grounding, the context block, then pins and summary, memory, skills,
    /// verbosity, response discipline, generated-image state, the
    /// supplied-arguments notice and any formatting guidance, and last the
    /// conversation.
    Legacy,
    /// The standing blocks first, then pins, summary and the prior
    /// conversation, then the per-turn blocks, then the current user turn.
    StablePrefix,
}

/// The chat prompt's parts before they are put in order. Each slot holds the
/// finished message; an empty slot sends nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PromptContext {
    /// The conversation-only scope notice.
    pub conversation_scope: Option<ChatMessage>,
    /// What this thread's artifacts are, at their current versions.
    pub artifact_inventory: Option<ChatMessage>,
    /// Platform, org, Space and agent-persona instructions.
    pub authored_instructions: Option<ChatMessage>,
    /// Who the model is talking to.
    pub identity: Option<ChatMessage>,
    /// Weekdays for the dates the user's messages name.
    pub calendar: Option<ChatMessage>,
    /// Today's date and how to treat time-sensitive facts.
    pub date_notice: Option<ChatMessage>,
    /// Retrieved evidence, or the notice that nothing was found.
    pub grounding: Option<ChatMessage>,
    /// Session-core's context block (system messages only).
    pub context_assembly: Vec<ChatMessage>,
    /// The thread as loaded: hoisted pins and any compaction summary first,
    /// then the conversation, ending with the current user turn.
    pub history: Vec<ChatMessage>,
    /// Recalled memories.
    pub memory: Option<ChatMessage>,
    /// The skills matched to this turn.
    pub skills: Option<ChatMessage>,
    /// The response-length directive.
    pub verbosity: Option<ChatMessage>,
    /// The standing response-discipline notice.
    pub response_discipline: Option<ChatMessage>,
    /// That the thread already produced an image.
    pub generated_image_state: Option<ChatMessage>,
    /// Guidance for tools whose arguments the user must supply.
    pub user_supplied_args: Option<ChatMessage>,
    /// Per-turn formatting guidance (the chart fence, later staged files and
    /// the deck path), present only on a turn whose latest user message asks
    /// for what it explains. Empty on every other turn, and an empty slot
    /// sends nothing: the prompt is then exactly what it was without the slot.
    pub format_guidance: Option<ChatMessage>,
}

impl PromptContext {
    /// The prompt's messages in `order`.
    #[must_use]
    pub fn render(self, order: PromptOrder) -> Vec<ChatMessage> {
        match order {
            PromptOrder::Legacy => self.render_legacy(),
            PromptOrder::StablePrefix => self.render_stable_prefix(),
        }
    }

    /// Byte-identical to the insertions the chat path made before these
    /// slots existed. Blocks it put at index 0 lead, in reverse insertion
    /// order; blocks it put at the first non-system position follow the
    /// context block and the history's own leading system messages (pins and
    /// summary), in insertion order.
    fn render_legacy(self) -> Vec<ChatMessage> {
        let Self {
            conversation_scope,
            artifact_inventory,
            authored_instructions,
            identity,
            calendar,
            date_notice,
            grounding,
            context_assembly,
            history,
            memory,
            skills,
            verbosity,
            response_discipline,
            generated_image_state,
            user_supplied_args,
            format_guidance,
        } = self;
        let leading = history
            .iter()
            .position(|message| message.role != "system")
            .unwrap_or(history.len());
        let mut history = history.into_iter();
        let mut out = Vec::new();
        out.extend(conversation_scope);
        out.extend(artifact_inventory);
        out.extend(authored_instructions);
        out.extend(identity);
        out.extend(calendar);
        out.extend(date_notice);
        out.extend(grounding);
        out.extend(context_assembly);
        out.extend(history.by_ref().take(leading));
        out.extend(memory);
        out.extend(skills);
        out.extend(verbosity);
        out.extend(response_discipline);
        out.extend(generated_image_state);
        out.extend(user_supplied_args);
        out.extend(format_guidance);
        out.extend(history);
        out
    }

    /// Standing blocks, then the prior conversation, then the per-turn
    /// blocks, then the current user turn.
    ///
    /// The standing blocks keep the order of the stable-first layout the
    /// cache investigation proposed; the date notice closes them because it
    /// is the one that changes daily. "The current user turn" is the last
    /// user message and anything after it, so a thread that somehow ends on
    /// another role still sends its per-turn blocks before its latest request.
    fn render_stable_prefix(self) -> Vec<ChatMessage> {
        let Self {
            conversation_scope,
            artifact_inventory,
            authored_instructions,
            identity,
            calendar,
            date_notice,
            grounding,
            context_assembly,
            mut history,
            memory,
            skills,
            verbosity,
            response_discipline,
            generated_image_state,
            user_supplied_args,
            format_guidance,
        } = self;
        let current_at = history
            .iter()
            .rposition(|message| message.role == "user")
            .unwrap_or(history.len());
        let current = history.split_off(current_at);
        let mut out = Vec::new();
        out.extend(conversation_scope);
        out.extend(authored_instructions);
        out.extend(identity);
        out.extend(response_discipline);
        out.extend(user_supplied_args);
        out.extend(verbosity);
        out.extend(date_notice);
        out.extend(history);
        out.extend(artifact_inventory);
        out.extend(calendar);
        out.extend(grounding);
        out.extend(context_assembly);
        out.extend(memory);
        out.extend(skills);
        out.extend(generated_image_state);
        out.extend(format_guidance);
        out.extend(current);
        out
    }
}

/// Tools offered on some turns only, in the order they close the tool list:
/// `reattach_context` once a thread has been compacted (and from then on),
/// `web_search` per query.
const CONDITIONAL_TOOLS: [&str; 2] = ["reattach_context", "web_search"];

/// `tools` with the conditional tools moved to the end.
///
/// The tools come first in an `OpenAI` prompt, so a tool that is offered on
/// one turn and withheld on the next changes every tool definition after it.
/// With the conditional ones last, the rest of the list stays one stable
/// prefix. Everything else keeps its order.
#[must_use]
pub fn conditional_tools_last(tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    let (mut ordered, conditional): (Vec<ToolDefinition>, Vec<ToolDefinition>) = tools
        .into_iter()
        .partition(|tool| !CONDITIONAL_TOOLS.contains(&tool.name.as_str()));
    for name in CONDITIONAL_TOOLS {
        ordered.extend(conditional.iter().filter(|tool| tool.name == name).cloned());
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(content: &str) -> ChatMessage {
        ChatMessage {
            role: "system".to_owned(),
            content: content.to_owned(),
            ..Default::default()
        }
    }

    fn turn(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_owned(),
            content: content.to_owned(),
            ..Default::default()
        }
    }

    fn first_non_system(messages: &[ChatMessage]) -> usize {
        messages
            .iter()
            .position(|message| message.role != "system")
            .unwrap_or(messages.len())
    }

    /// The chat path's prompt assembly as it was before `PromptContext`:
    /// the same insertions, in the same order, copied from `sse.rs`.
    fn legacy_insertions(context: &PromptContext) -> Vec<ChatMessage> {
        let mut messages: Vec<ChatMessage> = context.context_assembly.clone();
        messages.extend(context.history.iter().cloned());
        if let Some(memory) = &context.memory {
            let insert_at = first_non_system(&messages);
            messages.insert(insert_at, memory.clone());
        }
        if let Some(grounding) = &context.grounding {
            messages.insert(0, grounding.clone());
        }
        if let Some(date_notice) = &context.date_notice {
            messages.insert(0, date_notice.clone());
        }
        if let Some(calendar) = &context.calendar {
            messages.insert(0, calendar.clone());
        }
        if let Some(identity) = &context.identity {
            messages.insert(0, identity.clone());
        }
        if let Some(instructions) = &context.authored_instructions {
            messages.insert(0, instructions.clone());
        }
        for block in [
            &context.skills,
            &context.verbosity,
            &context.response_discipline,
            &context.generated_image_state,
        ]
        .into_iter()
        .flatten()
        {
            let insert_at = first_non_system(&messages);
            messages.insert(insert_at, block.clone());
        }
        if let Some(inventory) = &context.artifact_inventory {
            messages.insert(0, inventory.clone());
        }
        if let Some(scope) = &context.conversation_scope {
            messages.insert(0, scope.clone());
        }
        if let Some(notice) = &context.user_supplied_args {
            let insert_at = first_non_system(&messages);
            messages.insert(insert_at, notice.clone());
        }
        messages
    }

    fn full_context(history: Vec<ChatMessage>) -> PromptContext {
        PromptContext {
            conversation_scope: Some(system("scope")),
            artifact_inventory: Some(system("inventory v3")),
            authored_instructions: Some(system("instructions")),
            identity: Some(system("identity")),
            calendar: Some(system("calendar 24. september")),
            date_notice: Some(system("date notice")),
            grounding: Some(system("grounding for query A")),
            context_assembly: vec![system("context block, estimated tokens: 812")],
            history,
            memory: Some(system("memory recall A")),
            skills: Some(system("skills A")),
            verbosity: Some(system("verbosity")),
            response_discipline: Some(system("discipline")),
            generated_image_state: Some(system("image state")),
            user_supplied_args: Some(system("supplied args")),
            format_guidance: None,
        }
    }

    fn history_with_pins_and_summary() -> Vec<ChatMessage> {
        vec![
            system("[Pinned by the user] user: keep NOK"),
            system("Summary of the earlier part of this conversation\nNOK, order 4411"),
            turn("user", "first question"),
            turn("assistant", "first answer"),
            turn("user", "current question"),
        ]
    }

    #[test]
    fn the_legacy_order_is_byte_identical_to_the_old_insertions() {
        let empty = PromptContext::default();
        let histories = [
            Vec::new(),
            vec![turn("user", "only turn")],
            history_with_pins_and_summary(),
            // All-system history: every "first non-system" insert lands last.
            vec![system("pin"), system("summary")],
            // A system message inside the conversation stays where it was.
            vec![
                turn("user", "a"),
                system("mid-thread notice"),
                turn("assistant", "b"),
                turn("user", "c"),
            ],
        ];
        for history in histories {
            let full = full_context(history.clone());
            assert_eq!(
                full.clone().render(PromptOrder::Legacy),
                legacy_insertions(&full)
            );

            let sparse = PromptContext {
                date_notice: Some(system("date notice")),
                response_discipline: Some(system("discipline")),
                history: history.clone(),
                ..empty.clone()
            };
            assert_eq!(
                sparse.clone().render(PromptOrder::Legacy),
                legacy_insertions(&sparse)
            );

            let mut assembly_only = sparse.clone();
            assembly_only.context_assembly = vec![system("block one"), system("block two")];
            assembly_only.grounding = Some(system("no grounding notice"));
            assembly_only.memory = Some(system("memory"));
            assert_eq!(
                assembly_only.clone().render(PromptOrder::Legacy),
                legacy_insertions(&assembly_only)
            );
        }
    }

    #[test]
    fn the_legacy_order_matches_the_documented_table() {
        let rendered = full_context(history_with_pins_and_summary()).render(PromptOrder::Legacy);
        let contents: Vec<&str> = rendered.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            contents,
            [
                "scope",
                "inventory v3",
                "instructions",
                "identity",
                "calendar 24. september",
                "date notice",
                "grounding for query A",
                "context block, estimated tokens: 812",
                "[Pinned by the user] user: keep NOK",
                "Summary of the earlier part of this conversation\nNOK, order 4411",
                "memory recall A",
                "skills A",
                "verbosity",
                "discipline",
                "image state",
                "supplied args",
                "first question",
                "first answer",
                "current question",
            ]
        );
    }

    #[test]
    fn the_stable_prefix_order_puts_standing_blocks_first_and_per_turn_blocks_last() {
        let rendered =
            full_context(history_with_pins_and_summary()).render(PromptOrder::StablePrefix);
        let contents: Vec<&str> = rendered.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            contents,
            [
                "scope",
                "instructions",
                "identity",
                "discipline",
                "supplied args",
                "verbosity",
                "date notice",
                "[Pinned by the user] user: keep NOK",
                "Summary of the earlier part of this conversation\nNOK, order 4411",
                "first question",
                "first answer",
                "inventory v3",
                "calendar 24. september",
                "grounding for query A",
                "context block, estimated tokens: 812",
                "memory recall A",
                "skills A",
                "image state",
                "current question",
            ]
        );
    }

    #[test]
    fn both_orders_send_the_same_messages() {
        let context = full_context(history_with_pins_and_summary());
        let mut legacy = context.clone().render(PromptOrder::Legacy);
        let mut stable = context.render(PromptOrder::StablePrefix);
        let key = |m: &ChatMessage| (m.role.clone(), m.content.clone());
        legacy.sort_by_key(key);
        stable.sort_by_key(key);
        assert_eq!(legacy, stable);
    }

    /// The payoff: the next turn changes every per-turn block and adds an
    /// exchange, and its prompt still opens with every message the previous
    /// turn sent ahead of that turn's per-turn blocks, prior history included.
    #[test]
    fn a_later_turn_repeats_the_earlier_prefix_byte_for_byte() {
        let first = full_context(history_with_pins_and_summary());
        let mut next_history = history_with_pins_and_summary();
        next_history.push(turn("assistant", "current answer"));
        next_history.push(turn("user", "next question"));
        let next = PromptContext {
            artifact_inventory: Some(system("inventory v4")),
            calendar: Some(system("calendar 1. oktober")),
            grounding: Some(system("grounding for query B")),
            context_assembly: vec![system("context block, estimated tokens: 1204")],
            memory: Some(system("memory recall B")),
            skills: Some(system("skills B")),
            generated_image_state: None,
            history: next_history,
            ..first.clone()
        };
        let first = first.render(PromptOrder::StablePrefix);
        let next = next.render(PromptOrder::StablePrefix);
        let shared = first
            .iter()
            .position(|m| m.content == "inventory v3")
            .expect("per-turn blocks follow the prior history");
        assert_eq!(
            shared, 11,
            "7 standing blocks, pins, summary and one prior exchange"
        );
        assert_eq!(&next[..shared], &first[..shared]);
        assert_eq!(
            next[shared].content, "current question",
            "the old turn is now history"
        );
        assert_eq!(
            next.last().map(|m| m.content.as_str()),
            Some("next question")
        );

        // The legacy order shares only the blocks ahead of the inventory.
        let legacy_first =
            full_context(history_with_pins_and_summary()).render(PromptOrder::Legacy);
        assert_eq!(legacy_first[1].content, "inventory v3");
    }

    /// The formatting-guidance slot is a per-turn block like the others: it
    /// sits just ahead of the conversation in the legacy order and last among
    /// the per-turn blocks, before the current user turn, in the stable order.
    #[test]
    fn the_format_guidance_slot_sits_with_the_per_turn_blocks() {
        let with = PromptContext {
            format_guidance: Some(system("chart notice")),
            ..full_context(history_with_pins_and_summary())
        };

        let legacy = with.clone().render(PromptOrder::Legacy);
        let contents: Vec<&str> = legacy.iter().map(|m| m.content.as_str()).collect();
        let notice = contents.iter().position(|c| *c == "chart notice");
        let supplied = contents.iter().position(|c| *c == "supplied args");
        let first_turn = contents.iter().position(|c| *c == "first question");
        assert_eq!(notice, supplied.map(|at| at + 1));
        assert_eq!(notice.map(|at| at + 1), first_turn);

        let stable = with.render(PromptOrder::StablePrefix);
        let contents: Vec<&str> = stable.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            &contents[contents.len() - 3..],
            ["image state", "chart notice", "current question"]
        );
    }

    #[test]
    fn an_empty_format_guidance_slot_changes_nothing_in_either_order() {
        for order in [PromptOrder::Legacy, PromptOrder::StablePrefix] {
            let context = full_context(history_with_pins_and_summary());
            assert!(context.format_guidance.is_none());
            let rendered = context.render(order);
            assert!(rendered.iter().all(|m| m.content != "chart notice"));
        }
        // Sent with the slot filled, both orders still send the same messages.
        let filled = PromptContext {
            format_guidance: Some(system("chart notice")),
            ..full_context(history_with_pins_and_summary())
        };
        let mut legacy = filled.clone().render(PromptOrder::Legacy);
        let mut stable = filled.render(PromptOrder::StablePrefix);
        let key = |m: &ChatMessage| (m.role.clone(), m.content.clone());
        legacy.sort_by_key(key);
        stable.sort_by_key(key);
        assert_eq!(legacy, stable);
    }

    #[test]
    fn the_stable_prefix_order_keeps_a_turn_that_ends_on_another_role_last() {
        let context = PromptContext {
            memory: Some(system("memory")),
            history: vec![turn("user", "q"), turn("assistant", "partial")],
            ..PromptContext::default()
        };
        let contents: Vec<String> = context
            .render(PromptOrder::StablePrefix)
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(contents, ["memory", "q", "partial"]);
    }

    #[test]
    fn only_a_pinned_openai_chat_completions_route_is_eligible() {
        use CacheRoute::{OpenAiChatCompletions, Other};
        for (hint, connection, model, expected) in [
            ("openai", "", "gpt-4o-mini", OpenAiChatCompletions),
            ("azure-openai", "", "gpt-5-mini", OpenAiChatCompletions),
            (" Azure_OpenAI ", "", "deepseek-v3-2", OpenAiChatCompletions),
            ("azure", "", "", OpenAiChatCompletions),
            ("openai", "", "verevon-auto", OpenAiChatCompletions),
            // The recorded route and its aliases, with or without a model.
            (
                "openai-codex-subscription",
                "conn-1",
                "gpt-5.6-terra",
                Other,
            ),
            ("codex-subscription", "conn-1", "gpt-5.6-terra", Other),
            ("chatgpt-codex", "conn-1", "", Other),
            // A subscription connection decides even under an OpenAI hint.
            ("openai", "conn-1", "gpt-5.6-terra", Other),
            // Claude, by hint or by model.
            ("anthropic", "", "claude-sonnet-4-6", Other),
            ("azure-anthropic", "", "", Other),
            ("claude", "", "", Other),
            ("openai", "", "claude-haiku-4-5", Other),
            // Other providers, and a turn whose provider is chosen downstream.
            ("cohere", "", "cohere-command-a", Other),
            ("", "", "gpt-4o-mini", Other),
            ("", "", "", Other),
        ] {
            assert_eq!(
                CacheRoute::of_turn(hint, connection, model),
                expected,
                "{hint:?} / {connection:?} / {model:?}"
            );
        }
    }

    #[test]
    fn every_option_defaults_off_and_only_applies_on_an_eligible_route() {
        assert_eq!(
            CacheOptions::parse(None, None, None),
            CacheOptions::default()
        );
        assert_eq!(CacheOptions::default().prompt_order(), PromptOrder::Legacy);
        for off in ["", "0", "off", "false", "no", "maybe"] {
            let parsed = CacheOptions::parse(Some(off), Some("0"), Some(off));
            assert_eq!(parsed, CacheOptions::default(), "{off:?}");
        }
        let all = CacheOptions::parse(Some(" ON "), Some("12000"), Some("1"));
        assert_eq!(
            all,
            CacheOptions {
                stable_prompt_order: true,
                compaction_token_budget: Some(12_000),
                dedup_context_history: true,
            }
        );
        assert_eq!(all.prompt_order(), PromptOrder::StablePrefix);
        assert_eq!(all.for_route(CacheRoute::OpenAiChatCompletions), all);
        assert_eq!(all.for_route(CacheRoute::Other), CacheOptions::default());
        // A set budget is kept inside the bounds; nonsense leaves it unset.
        assert_eq!(
            CacheOptions::parse(None, Some("5"), None).compaction_token_budget,
            Some(1_000)
        );
        assert_eq!(
            CacheOptions::parse(None, Some("9999999"), None).compaction_token_budget,
            Some(200_000)
        );
        assert_eq!(
            CacheOptions::parse(None, Some("-3"), None).compaction_token_budget,
            None
        );
        assert_eq!(
            CacheOptions::parse(None, Some("lots"), None).compaction_token_budget,
            None
        );
    }

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn conditional_tools_move_to_the_end_and_the_rest_keep_their_order() {
        let names = |tools: Vec<ToolDefinition>| -> Vec<String> {
            tools.into_iter().map(|t| t.name).collect()
        };
        let offered = vec![
            tool("client_lookup"),
            tool("count_words"),
            tool("web_search"),
            tool("knowledge_search"),
            tool("reattach_context"),
            tool("fetch_url"),
        ];
        assert_eq!(
            names(conditional_tools_last(offered)),
            [
                "client_lookup",
                "count_words",
                "knowledge_search",
                "fetch_url",
                "reattach_context",
                "web_search"
            ]
        );
        // With web_search withheld, everything before it is unchanged.
        let with = names(conditional_tools_last(vec![
            tool("a"),
            tool("web_search"),
            tool("b"),
        ]));
        let without = names(conditional_tools_last(vec![tool("a"), tool("b")]));
        assert_eq!(&with[..2], without.as_slice());
    }
}
