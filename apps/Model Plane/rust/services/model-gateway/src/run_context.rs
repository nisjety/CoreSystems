//! The run context a Do, Plan or execute run starts with (build plan WS02-S6).
//!
//! An Ask turn is told who it is answering for (platform, org, Space and persona
//! instructions), what was said earlier in the thread and what the user's memory
//! holds. The agentic branch of `invoke_stream_sse` returned before any of that,
//! so a Do run began from its goal alone. This module composes the same three
//! things for `RunAgentRequest.run_context` (execution.proto field 18):
//!
//! * the authored-instruction text, from the one composer an Ask turn also uses
//!   ([`compose_authored_instructions`]), so the two loops cannot drift;
//! * bounded history, cut to a token budget with no summariser call
//!   ([`bound_history`]);
//! * a memory block, fetched under the Ask path's 400 ms budget and skipped for a
//!   trivial prompt and for a Space-scoped run.
//!
//! The composition is data only. It never touches `mode`, `plan_mode` or the
//! autonomy rung, which the gateway resolves on its own; instruction text is
//! advisory framing, never authority. What was and was not included is stamped on
//! the run record as closed tokens and counts (`RecordRunContext`), never text.
//!
//! The switch is `MODEL_GATEWAY_RUN_CONTEXT` (`off`, `instructions` or `full`,
//! default off). Off sends no field and makes no extra call, so a run is
//! byte-identical to one built before this module existed.

use mp_contracts::model_plane::v1::{
    ChatMessage, ListConversationRequest, RecordRunContextRequest, RunContext, RunContextPart,
    RunContextState, RunContextStatus, SearchMemoryRequest,
};
use mp_contracts::run_context::{
    MAX_AUTHORED_INSTRUCTION_BYTES, MAX_HISTORY_BYTES, MAX_HISTORY_MESSAGES, MAX_MEMORY_BYTES,
};

use crate::auth::VerifiedSessionBearer;
use crate::session_flow::SessionRun;
use crate::state::AppState;

/// The switch: `off`, `instructions` or `full`.
pub const MODE_ENV: &str = "MODEL_GATEWAY_RUN_CONTEXT";
/// History budget for `full`, in estimated tokens.
pub const HISTORY_TOKENS_ENV: &str = "MODEL_GATEWAY_RUN_CONTEXT_HISTORY_TOKENS";
const DEFAULT_HISTORY_TOKENS: usize = 8_000;
const MIN_HISTORY_TOKENS: usize = 1_000;
const MAX_HISTORY_TOKENS: usize = 40_000;

/// Bound on the history read. The run is not dispatched until it returns, so a
/// slow store must cost the history, not the run.
const HISTORY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
/// Bound on the persona-toggle read. It runs beside the history and memory reads
/// inside one `join!`, so without its own bound a stalled context-assembly RPC
/// would hold the run however fast the other two finish. Past it the toggle is
/// open, like an Ask turn whose assembly is unavailable.
const PERSONA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
/// Bound on stamping the status, for the same reason.
const RECORD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Closed reasons a part carries on the run record. Never prose.
pub const REASON_ZDR: &str = "zdr";
pub const REASON_SPACE_SCOPE: &str = "space_scope";
pub const REASON_ROUTE_UNCACHED: &str = "route_uncached";
pub const REASON_TRIVIAL_PROMPT: &str = "trivial_prompt";
pub const REASON_TIMEOUT: &str = "timeout";
pub const REASON_NO_CREDENTIAL: &str = "no_credential";
pub const REASON_ERROR: &str = "error";
/// The contract's message/byte caps cut the part.
pub const REASON_CAP: &str = "cap";
/// The token-budget planner cut the history (the contract caps cut nothing more).
pub const REASON_BUDGET: &str = "budget";

/// How much of the Ask run context a run is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunContextMode {
    /// Nothing: the request carries no `run_context` and no call is made.
    Off,
    /// The authored instructions only.
    Instructions,
    /// Instructions, bounded history and memory.
    Full,
}

impl RunContextMode {
    /// Strict: only the exact lowercase literals enable. Any other value
    /// (`FULL`, `on`, `true`, a typo) is off, so a mistake never widens what a
    /// run is told.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "instructions" => Self::Instructions,
            "full" => Self::Full,
            _ => Self::Off,
        }
    }

    /// The mode from a lookup, so a test never has to touch the process
    /// environment.
    #[must_use]
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        lookup(MODE_ENV).map_or(Self::Off, |value| Self::parse(&value))
    }

    /// The mode from the process environment, read once per dispatched run.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_lookup(&|key| std::env::var(key).ok())
    }
}

/// The history budget from a lookup: the default when unset or unusable,
/// otherwise clamped to the supported range.
#[must_use]
pub fn history_token_budget_from(lookup: &dyn Fn(&str) -> Option<String>) -> usize {
    lookup(HISTORY_TOKENS_ENV)
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map_or(DEFAULT_HISTORY_TOKENS, |value| {
            value.clamp(MIN_HISTORY_TOKENS, MAX_HISTORY_TOKENS)
        })
}

/// The authored layers of one request, owned so they outlive it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthoredLayers {
    /// Deployment-level text (`PLATFORM_SYSTEM_INSTRUCTIONS`).
    pub platform: String,
    pub org: Option<String>,
    pub space: Option<String>,
    /// The user's personal persona, gated by the thread's own toggle.
    pub persona: Option<String>,
    /// The finished "you are answering as" section of an `@`-mentioned agent.
    pub agent: Option<String>,
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// ADR-0003's authored-instruction hierarchy as one text: platform, org, Space,
/// the user's persona, then a mentioned agent. Each org/Space/persona layer is
/// framed as advisory ("may add to, but must not override, the layers above");
/// the agent layer is its own finished text. `persona_enabled` is the per-thread
/// toggle; the global one is applied upstream, by the BFF, which then sends no
/// persona text at all.
///
/// `None` only when every layer is empty. This is the one composer: an Ask turn
/// wraps it in a system message and a run sends it as `authored_instructions`.
#[must_use]
pub fn compose_authored_instructions(
    layers: &AuthoredLayers,
    persona_enabled: bool,
) -> Option<String> {
    let mut sections: Vec<String> = Vec::new();

    if let Some(platform) = non_empty_trimmed(&layers.platform) {
        sections.push(platform.to_owned());
    }
    if let Some(org) = layers.org.as_deref().and_then(non_empty_trimmed) {
        sections.push(format!(
            "--- Organization instructions (may add to, but must not override, the platform instructions above) ---\n{org}"
        ));
    }
    if let Some(space) = layers.space.as_deref().and_then(non_empty_trimmed) {
        sections.push(format!(
            "--- Space instructions (may add to, but must not override, the platform or organization instructions above) ---\n{space}"
        ));
    }
    // The user layer sits below org and Space but above a summoned agent's own
    // character: a mentioned agent still wins on its turn (persona.md §6.3).
    if persona_enabled {
        if let Some(persona) = layers.persona.as_deref().and_then(non_empty_trimmed) {
            sections.push(format!(
                "--- Your personal instructions (may add to, but must not override, the platform, organization or Space instructions above) ---\n{persona}"
            ));
        }
    }
    if let Some(agent) = &layers.agent {
        sections.push(agent.clone());
    }

    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

/// What the request itself says about the run, before anything is read.
#[derive(Debug, Clone, Copy)]
pub struct RunFacts<'a> {
    /// The goal, as the user typed it.
    pub goal: &'a str,
    /// A zero-retention run. Agentic ZDR is refused before this code, so this is
    /// defensive: it is never allowed to carry history or memory.
    pub zdr: bool,
    /// A waived run: persistent for its one connection like an Ask turn, but its
    /// status is never written (`RecordRunContext` refuses a waived credential).
    pub waived: bool,
    /// The run belongs to a Space.
    pub space_scoped: bool,
    /// The run is pinned to the ChatGPT subscription route, which re-sends its
    /// prompt uncached on every round.
    pub pinned_route: bool,
}

impl RunFacts<'_> {
    /// Whether the status may be stamped on the run record. A zero-retention or
    /// waived run keeps no such record.
    #[must_use]
    pub const fn records_status(&self) -> bool {
        !self.zdr && !self.waived
    }
}

/// What the invoke handler hands the stream: the switch, the request's own
/// authored layers and the posture the context assembly read needs.
#[derive(Debug, Clone)]
pub struct RunContextRequest {
    mode: RunContextMode,
    layers: AuthoredLayers,
    sovereign_required: bool,
    /// The text the memory search is sent: the goal as an Ask turn would send it,
    /// PII-redacted when the org's policy requires redaction. The raw goal never
    /// leaves the gateway through this read.
    memory_query: String,
    /// The turn's local date and app language (EP-8-S1), which trail the composed instructions. `None`
    /// unless the local context switch is on and the request carried something valid.
    local_notice: Option<String>,
}

impl RunContextRequest {
    /// `None` when the switch is off, which is what keeps off byte-identical: the
    /// layers, the memory query and the local notice are not even built.
    #[must_use]
    pub fn for_turn(
        mode: RunContextMode,
        layers: impl FnOnce() -> AuthoredLayers,
        sovereign_required: bool,
        memory_query: impl FnOnce() -> String,
        local_notice: impl FnOnce() -> Option<String>,
    ) -> Option<Self> {
        (mode != RunContextMode::Off).then(|| Self {
            mode,
            layers: layers(),
            sovereign_required,
            memory_query: memory_query(),
            local_notice: local_notice(),
        })
    }
}

/// What to do about one optional part of the context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    /// The mode does not ask for it; the record leaves it unstated.
    NotRequested,
    /// Policy withholds it, for this closed reason.
    Skip(&'static str),
    /// Read it.
    Load,
}

fn plan_history(mode: RunContextMode, facts: &RunFacts<'_>) -> Plan {
    if mode != RunContextMode::Full {
        Plan::NotRequested
    } else if facts.zdr {
        Plan::Skip(REASON_ZDR)
    } else if facts.pinned_route {
        Plan::Skip(REASON_ROUTE_UNCACHED)
    } else {
        Plan::Load
    }
}

fn plan_memory(mode: RunContextMode, facts: &RunFacts<'_>) -> Plan {
    match plan_history(mode, facts) {
        Plan::Load if facts.space_scoped => Plan::Skip(REASON_SPACE_SCOPE),
        Plan::Load if crate::memory_prefetch::is_trivial_prompt(facts.goal) => {
            Plan::Skip(REASON_TRIVIAL_PROMPT)
        }
        other => other,
    }
}

/// One optional part as the composer receives it.
#[derive(Debug, Clone, PartialEq)]
pub enum Part<T> {
    NotRequested,
    Skipped(&'static str),
    Loaded(T),
    Unavailable(&'static str),
}

impl<T> Part<T> {
    fn from_plan(plan: Plan, load: Option<Result<T, &'static str>>) -> Self {
        match (plan, load) {
            (Plan::NotRequested, _) => Self::NotRequested,
            (Plan::Skip(reason), _) => Self::Skipped(reason),
            (Plan::Load, Some(Ok(value))) => Self::Loaded(value),
            (Plan::Load, Some(Err(reason))) => Self::Unavailable(reason),
            (Plan::Load, None) => Self::Unavailable(REASON_ERROR),
        }
    }
}

/// The history after it was bounded.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundedHistory {
    pub messages: Vec<ChatMessage>,
    /// Prior turns kept (the leading notice, when there is one, is not a turn).
    pub included: usize,
    /// Prior turns left out.
    pub omitted: usize,
    /// Why turns were left out, as the closed token the record carries: empty when
    /// none were, [`REASON_CAP`] when the contract's message or byte caps cut
    /// anything (they are the hard limit, so they win when both applied) and
    /// [`REASON_BUDGET`] when only the token-budget planner did.
    pub reason: &'static str,
}

/// The prior turns of a thread, bounded for a run.
///
/// The thread's last user turn is the goal the run was started with, which the
/// run already carries, so it is dropped. Only user and assistant turns remain.
/// The tail is cut to `budget_tokens` with the planner an Ask turn uses (no
/// summariser call: it runs before dispatch), then to the contract's message and
/// byte caps, oldest first. Whatever was cut leaves one honest notice at the head.
#[must_use]
pub fn bound_history(prior: Vec<ChatMessage>, budget_tokens: usize) -> BoundedHistory {
    let mut turns: Vec<ChatMessage> = prior
        .into_iter()
        .filter(|message| {
            matches!(message.role.as_str(), "user" | "assistant")
                && !message.content.trim().is_empty()
        })
        .collect();
    if turns.last().is_some_and(|message| message.role == "user") {
        turns.pop();
    }
    let total = turns.len();

    let mut noticed = false;
    let mut budget_cut = false;
    if let Some(head) = crate::compaction::plan_head_summary_by_budget(&turns, budget_tokens) {
        turns = crate::compaction::apply_head_drop(turns, head);
        noticed = true;
        budget_cut = true;
    }
    let body = &turns[usize::from(noticed)..];
    let notice_bytes = crate::compaction::RUN_HISTORY_DROPPED_NOTICE.len();

    let mut first = fit_caps(
        body,
        MAX_HISTORY_MESSAGES - usize::from(noticed),
        MAX_HISTORY_BYTES - if noticed { notice_bytes } else { 0 },
    );
    if first > 0 && !noticed {
        noticed = true;
        first = fit_caps(
            body,
            MAX_HISTORY_MESSAGES - 1,
            MAX_HISTORY_BYTES - notice_bytes,
        );
    }

    let cap_cut = first > 0;
    let kept = &body[first..];
    let mut messages = Vec::with_capacity(kept.len() + usize::from(noticed));
    if noticed {
        messages.push(crate::compaction::run_history_dropped_notice());
    }
    messages.extend_from_slice(kept);
    let omitted = total - kept.len();
    BoundedHistory {
        messages,
        included: kept.len(),
        omitted,
        reason: if omitted == 0 {
            ""
        } else if cap_cut {
            REASON_CAP
        } else if budget_cut {
            REASON_BUDGET
        } else {
            // Unreachable by construction (turns leave only by one of the two
            // cuts); stated as the hard limit rather than left blank.
            REASON_CAP
        },
    }
}

/// Index of the oldest message of the newest run of `body` that fits both caps.
fn fit_caps(body: &[ChatMessage], max_messages: usize, max_bytes: usize) -> usize {
    let (mut count, mut bytes, mut first) = (0usize, 0usize, body.len());
    for (index, message) in body.iter().enumerate().rev() {
        if count == max_messages || bytes + message.content.len() > max_bytes {
            break;
        }
        count += 1;
        bytes += message.content.len();
        first = index;
    }
    first
}

fn part(state: RunContextState, included: usize, omitted: usize, reason: &str) -> RunContextPart {
    RunContextPart {
        state: state as i32,
        included: u32::try_from(included).unwrap_or(u32::MAX),
        omitted: u32::try_from(omitted).unwrap_or(u32::MAX),
        reason: reason.to_owned(),
    }
}

/// The longest prefix of `text` within `max_bytes` that ends on a char boundary.
fn within_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Everything [`compose`] needs, already read.
#[derive(Debug)]
pub struct Composition<'a> {
    pub layers: &'a AuthoredLayers,
    /// The thread's own persona toggle.
    pub persona_enabled: bool,
    pub history_budget_tokens: usize,
    /// The thread as persisted, current turn last.
    pub history: Part<Vec<ChatMessage>>,
    pub memory: Part<Vec<String>>,
    /// The turn's local date and app language, placed after the instructions (EP-8-S1).
    pub local_notice: Option<String>,
}

/// The Do preamble: the composed instructions with the turn's local notice after them. The notice moves
/// with the clock, so it trails the text that stays the same for the person.
fn with_local_notice(text: Option<String>, notice: Option<String>) -> Option<String> {
    match (text, notice) {
        (text, None) => text,
        (Some(text), Some(notice)) => Some(format!("{text}\n\n{notice}")),
        (None, Some(notice)) => Some(notice),
    }
}

/// Build the run context and its status. Pure: every read has happened.
#[must_use]
pub fn compose(input: Composition<'_>) -> RunContext {
    let mut status = RunContextStatus::default();

    let mut authored_instructions = String::new();
    status.instructions = Some(
        match with_local_notice(
            compose_authored_instructions(input.layers, input.persona_enabled),
            input.local_notice,
        ) {
            None => part(RunContextState::Empty, 0, 0, ""),
            Some(text) if text.len() <= MAX_AUTHORED_INSTRUCTION_BYTES => {
                authored_instructions = text;
                part(RunContextState::Included, 1, 0, "")
            }
            Some(text) => {
                // Over the contract's cap the whole run would be refused at the
                // executor, so the tail is cut and the cut is stated (the unit
                // of `omitted` is bytes for this part).
                let kept = within_bytes(&text, MAX_AUTHORED_INSTRUCTION_BYTES);
                let omitted = text.len() - kept.len();
                authored_instructions = kept.to_owned();
                part(RunContextState::Truncated, 1, omitted, REASON_CAP)
            }
        },
    );

    let mut history = Vec::new();
    status.history = match input.history {
        Part::NotRequested => None,
        Part::Skipped(reason) => Some(part(RunContextState::SkippedByPolicy, 0, 0, reason)),
        Part::Unavailable(reason) => Some(part(RunContextState::Unavailable, 0, 0, reason)),
        Part::Loaded(prior) => {
            let bounded = bound_history(prior, input.history_budget_tokens);
            let state = if bounded.omitted > 0 {
                RunContextState::Truncated
            } else if bounded.included == 0 {
                RunContextState::Empty
            } else {
                RunContextState::Included
            };
            let counted = part(state, bounded.included, bounded.omitted, bounded.reason);
            history = bounded.messages;
            Some(counted)
        }
    };

    let mut memory_block = String::new();
    status.memory = match input.memory {
        Part::NotRequested => None,
        Part::Skipped(reason) => Some(part(RunContextState::SkippedByPolicy, 0, 0, reason)),
        Part::Unavailable(reason) => Some(part(RunContextState::Unavailable, 0, 0, reason)),
        Part::Loaded(entries) => {
            // Fewest entries dropped, lowest ranked first, until the block fits.
            let mut kept = entries.len();
            let block = loop {
                match crate::memory_provenance::memory_context_block(&entries[..kept]) {
                    Some(block) if block.len() > MAX_MEMORY_BYTES && kept > 0 => kept -= 1,
                    Some(block) if block.len() <= MAX_MEMORY_BYTES => break Some(block),
                    _ => break None,
                }
            };
            let included = if block.is_some() { kept } else { 0 };
            let omitted = entries.len() - included;
            memory_block = block.unwrap_or_default();
            Some(if entries.is_empty() {
                part(RunContextState::Empty, 0, 0, "")
            } else if omitted > 0 {
                part(RunContextState::Truncated, included, omitted, REASON_CAP)
            } else {
                part(RunContextState::Included, included, 0, "")
            })
        }
    };

    RunContext {
        authored_instructions,
        history,
        memory_block,
        status: Some(status),
    }
}

/// The thread as persisted, or the closed reason it could not be read.
async fn load_history(
    state: &AppState,
    org_id: &str,
    thread_id: &str,
    bearer: &VerifiedSessionBearer,
) -> Result<Vec<ChatMessage>, &'static str> {
    let Ok(request) = crate::sse::authenticated_session_request(
        ListConversationRequest {
            org_id: org_id.to_owned(),
            thread_id: thread_id.to_owned(),
            // The caller's own thread: no shared-Space read authority is involved.
            space_id: String::new(),
            space_read_decision_ref: String::new(),
            space_read_decision_token: String::new(),
            include_inactive: false,
            // Unpaged (WS09-S8): the whole conversation in sequence order.
            limit: 0,
            before_message_id: String::new(),
            ..Default::default()
        },
        bearer,
    ) else {
        return Err(REASON_NO_CREDENTIAL);
    };
    match tokio::time::timeout(
        HISTORY_TIMEOUT,
        state.session_client.clone().list_conversation(request),
    )
    .await
    {
        Ok(Ok(response)) => Ok(response
            .into_inner()
            .messages
            .into_iter()
            .map(|message| ChatMessage {
                compaction_summary: String::new(),
                role: message.role,
                content: message.content,
                name: String::new(),
            })
            .collect()),
        Ok(Err(error)) => {
            tracing::warn!(%error, %thread_id, "run context: thread history unavailable; the run starts without it");
            Err(REASON_ERROR)
        }
        Err(_elapsed) => {
            tracing::warn!(%thread_id, "run context: thread history timed out; the run starts without it");
            Err(REASON_TIMEOUT)
        }
    }
}

/// The memory recalled for the goal, under the Ask path's budget
/// (`MEMORY_PREFETCH_TIMEOUT`), or the closed reason it could not be read.
/// `query` is the redaction-aware text from [`RunContextRequest`], never the raw goal.
///
/// The same search an Ask turn makes, but it must tell "nothing to recall" from
/// "could not look": a run's record states which, and `fetch_chat_memory_context`
/// folds both into an empty answer. Counted on the same outcome metric.
async fn load_memory(
    state: &AppState,
    org_id: &str,
    thread_id: &str,
    query: &str,
    bearer: &VerifiedSessionBearer,
) -> Result<Vec<String>, &'static str> {
    let Ok(request) = crate::sse::authenticated_session_request(
        SearchMemoryRequest {
            // Left empty deliberately: session-core derives the owner from the
            // VERIFIED thread, so a caller-supplied user id would be forgeable scoping.
            user_id: String::new(),
            thread_id: thread_id.to_owned(),
            query: query.to_owned(),
            topic_filter: Vec::new(),
            limit: 5,
            org_id: org_id.to_owned(),
            updated_after: None,
        },
        bearer,
    ) else {
        crate::sse::record_memory_prefetch_outcome("no_credential");
        return Err(REASON_NO_CREDENTIAL);
    };
    match tokio::time::timeout(
        crate::sse::MEMORY_PREFETCH_TIMEOUT,
        state.memory_client.clone().search_memory(request),
    )
    .await
    {
        Ok(Ok(response)) => {
            let entries: Vec<String> = response
                .into_inner()
                .entries
                .into_iter()
                .map(|entry| entry.content)
                .filter(|content| !content.trim().is_empty())
                .collect();
            crate::sse::record_memory_prefetch_outcome(if entries.is_empty() {
                "empty"
            } else {
                "hit"
            });
            Ok(entries)
        }
        Ok(Err(error)) => {
            tracing::warn!(%error, "run context: memory recall failed; the run starts without it");
            crate::sse::record_memory_prefetch_outcome("error");
            Err(REASON_ERROR)
        }
        Err(_elapsed) => {
            tracing::warn!("run context: memory recall timed out; the run starts without it");
            crate::sse::record_memory_prefetch_outcome("timeout");
            Err(REASON_TIMEOUT)
        }
    }
}

/// Stamp how the context was assembled on the run record. Best-effort by
/// contract: a run must not fail because its record could not be annotated, so a
/// failure is logged and the run goes on.
pub async fn record_status(
    state: &AppState,
    org_id: &str,
    run_id: &str,
    bearer: &VerifiedSessionBearer,
    status: &RunContextStatus,
) {
    let Ok(request) = crate::sse::authenticated_session_request(
        RecordRunContextRequest {
            run_id: run_id.to_owned(),
            org_id: org_id.to_owned(),
            status: Some(status.clone()),
        },
        bearer,
    ) else {
        tracing::warn!(%run_id, "run context: session credential is not forwardable; status not recorded");
        return;
    };
    match tokio::time::timeout(
        RECORD_TIMEOUT,
        state.session_client.clone().record_run_context(request),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, %run_id, "run context: status not recorded (best-effort)");
        }
        Err(_elapsed) => {
            tracing::warn!(%run_id, "run context: recording the status timed out (best-effort)");
        }
    }
}

/// Read what the mode asks for, compose it and stamp the status.
///
/// The three reads run together: the thread's persona toggle (only when persona
/// text exists to gate), the thread history and the memory recall. None of them
/// can fail the run: each failure becomes an `UNAVAILABLE` part with a closed
/// reason and the run starts without it.
pub async fn prepare(
    state: &AppState,
    request: &RunContextRequest,
    facts: &RunFacts<'_>,
    run: &SessionRun,
    org_id: &str,
    bearer: &VerifiedSessionBearer,
) -> RunContext {
    let history_plan = plan_history(request.mode, facts);
    let memory_plan = plan_memory(request.mode, facts);

    let persona_toggle = async {
        if request.layers.persona.is_none() || facts.zdr {
            // Nothing to gate, or no read allowed: open, like an Ask turn whose
            // assembly is unavailable.
            return true;
        }
        // Bounded on its own: this future sits in the same `join!` as the other two
        // reads, so a stalled context-assembly RPC would otherwise hold the whole
        // run however fast they finish. A timeout leaves the toggle open, the same
        // default the read itself uses on an error.
        match tokio::time::timeout(
            PERSONA_TIMEOUT,
            crate::sse::load_thread_persona_enabled(
                state,
                &run.thread_id,
                &run.run_id,
                facts.goal,
                bearer,
                request.sovereign_required,
            ),
        )
        .await
        {
            Ok(enabled) => enabled,
            Err(_elapsed) => {
                tracing::warn!(thread_id = %run.thread_id, "run context: persona toggle read timed out; treated as on");
                true
            }
        }
    };
    let history = async {
        if history_plan == Plan::Load {
            Some(load_history(state, org_id, &run.thread_id, bearer).await)
        } else {
            None
        }
    };
    let memory = async {
        if memory_plan == Plan::Load {
            // The request's own query, never the raw goal: it is the PII-redacted
            // text when the org's policy requires it, as on the Ask path.
            Some(load_memory(state, org_id, &run.thread_id, &request.memory_query, bearer).await)
        } else {
            None
        }
    };
    let (persona_enabled, history, memory) = tokio::join!(persona_toggle, history, memory);

    let composed = compose(Composition {
        layers: &request.layers,
        persona_enabled,
        history_budget_tokens: history_token_budget_from(&|key| std::env::var(key).ok()),
        history: Part::from_plan(history_plan, history),
        memory: Part::from_plan(memory_plan, memory),
        local_notice: request.local_notice.clone(),
    });

    let status = composed.status.clone().unwrap_or_default();
    tracing::info!(
        run_id = %run.run_id,
        status = %mp_contracts::run_context::status_json(&status),
        "run context composed"
    );
    if facts.records_status() {
        record_status(state, org_id, &run.run_id, bearer, &status).await;
    }
    composed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_routes::InvokeRequest;

    fn message(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            compaction_summary: String::new(),
            role: role.to_owned(),
            content: content.to_owned(),
            name: String::new(),
        }
    }

    fn layers() -> AuthoredLayers {
        AuthoredLayers {
            platform: "Platform rule.".to_owned(),
            org: Some("Org rule.".to_owned()),
            space: Some("Space rule.".to_owned()),
            persona: Some("Always be terse.".to_owned()),
            agent: Some("You are currently answering as Driftsassistent.".to_owned()),
        }
    }

    fn facts(goal: &str) -> RunFacts<'_> {
        RunFacts {
            goal,
            zdr: false,
            waived: false,
            space_scoped: false,
            pinned_route: false,
        }
    }

    fn composition<'a>(
        layers: &'a AuthoredLayers,
        history: Part<Vec<ChatMessage>>,
        memory: Part<Vec<String>>,
    ) -> Composition<'a> {
        Composition {
            layers,
            persona_enabled: true,
            history_budget_tokens: DEFAULT_HISTORY_TOKENS,
            history,
            memory,
            local_notice: None,
        }
    }

    fn state_of(
        context: &RunContext,
        part_of: fn(&RunContextStatus) -> &Option<RunContextPart>,
    ) -> (RunContextState, u32, u32, String) {
        let status = context.status.as_ref().expect("status");
        let part = part_of(status).as_ref().expect("part stated");
        (
            RunContextState::try_from(part.state).expect("known state"),
            part.included,
            part.omitted,
            part.reason.clone(),
        )
    }

    fn history_of(status: &RunContextStatus) -> &Option<RunContextPart> {
        &status.history
    }

    fn memory_of(status: &RunContextStatus) -> &Option<RunContextPart> {
        &status.memory
    }

    fn instructions_of(status: &RunContextStatus) -> &Option<RunContextPart> {
        &status.instructions
    }

    /// The Do preamble carries the turn's local notice after the instructions, and alone when the turn has
    /// none. With no notice the text is exactly what it was (EP-8-S1).
    #[test]
    fn the_local_notice_trails_the_instructions() {
        let layers = layers();
        let bare = AuthoredLayers::default();
        let text = |layers: &AuthoredLayers, notice: Option<&str>| {
            let mut input = composition(
                layers,
                Part::from_plan(Plan::NotRequested, None),
                Part::from_plan(Plan::NotRequested, None),
            );
            input.local_notice = notice.map(str::to_owned);
            compose(input).authored_instructions
        };
        let base = text(&layers, None);
        assert_eq!(
            base,
            compose_authored_instructions(&layers, true).expect("the layers compose"),
        );
        assert_eq!(
            text(&layers, Some("Local notice.")),
            format!("{base}\n\nLocal notice."),
        );
        assert_eq!(text(&bare, None), "");
        assert_eq!(text(&bare, Some("Local notice.")), "Local notice.");
    }

    #[tokio::test]
    async fn composes_the_same_instruction_text_as_an_ask_turn() {
        let mut state = AppState::new();
        state.platform_instructions = "Platform rule.".to_owned();
        let req: InvokeRequest = serde_json::from_value(serde_json::json!({
            "content": "hva er kodenavnet?",
            "org_instructions": "Org rule.",
            "space_instructions": "Space rule.",
            "persona_instructions": "Always be terse.",
            "agent_name": "Driftsassistent",
            "agent_system_prompt": "Agent rule.",
        }))
        .expect("a valid invoke request");

        let ask = crate::sse::authored_instructions_message(&state, &req, true)
            .expect("an Ask turn composes instructions")
            .content;
        let run_layers = crate::sse::authored_layers(&state, &req);
        let run = compose(composition(
            &run_layers,
            Part::NotRequested,
            Part::NotRequested,
        ));

        assert_eq!(run.authored_instructions, ask, "one composer, two loops");
        assert_eq!(
            state_of(&run, instructions_of),
            (RunContextState::Included, 1, 0, String::new())
        );
    }

    #[test]
    fn precedence_is_platform_org_space_persona_agent() {
        let text = compose_authored_instructions(&layers(), true).expect("text");
        let at = |needle: &str| {
            text.find(needle)
                .unwrap_or_else(|| panic!("{needle} missing"))
        };
        assert!(at("Platform rule.") < at("Org rule."));
        assert!(at("Org rule.") < at("Space rule."));
        assert!(at("Space rule.") < at("Always be terse."));
        assert!(at("Always be terse.") < at("answering as Driftsassistent"));
        assert!(text.contains("must not override"));
    }

    #[test]
    fn persona_off_thread_omits_persona() {
        let text = compose_authored_instructions(&layers(), false).expect("text");
        assert!(!text.contains("Always be terse."), "{text}");
        assert!(text.contains("Org rule.") && text.contains("answering as Driftsassistent"));
        // And nothing at all when the persona was the only layer.
        let only = AuthoredLayers {
            persona: Some("Always be terse.".to_owned()),
            ..AuthoredLayers::default()
        };
        assert_eq!(compose_authored_instructions(&only, false), None);
    }

    #[test]
    fn history_drops_the_current_user_turn() {
        let prior = vec![
            message("user", "my project codename is BLUEHERON"),
            message("assistant", "Noted."),
            message("user", "what is my codename?"),
        ];
        let bounded = bound_history(prior, DEFAULT_HISTORY_TOKENS);
        assert_eq!(
            bounded.messages,
            vec![
                message("user", "my project codename is BLUEHERON"),
                message("assistant", "Noted."),
            ]
        );
        assert_eq!((bounded.included, bounded.omitted), (2, 0));

        // Only user and assistant turns are ever sent, and blank ones are not.
        let mixed = vec![
            message("system", "identity"),
            message("user", "   "),
            message("assistant", "hello"),
            message("user", "goal"),
        ];
        assert_eq!(
            bound_history(mixed, DEFAULT_HISTORY_TOKENS).messages,
            vec![message("assistant", "hello")]
        );
        // A thread with only the current turn has no history.
        let alone = bound_history(vec![message("user", "goal")], DEFAULT_HISTORY_TOKENS);
        assert_eq!(
            (alone.messages.len(), alone.included, alone.omitted),
            (0, 0, 0)
        );
        let context = compose(composition(
            &AuthoredLayers::default(),
            Part::Loaded(vec![message("user", "goal")]),
            Part::NotRequested,
        ));
        assert_eq!(
            state_of(&context, history_of),
            (RunContextState::Empty, 0, 0, String::new())
        );
    }

    #[test]
    fn history_tail_respects_the_token_budget_and_leaves_an_honest_notice() {
        let filler = "x".repeat(2_000);
        let mut prior = Vec::new();
        for index in 0..20 {
            prior.push(message("user", &format!("question {index} {filler}")));
            prior.push(message("assistant", &format!("answer {index} {filler}")));
        }
        prior.push(message("user", "the goal"));
        let budget = 2_000;

        let bounded = bound_history(prior, budget);

        let notice = crate::compaction::run_history_dropped_notice();
        assert_eq!(bounded.messages[0], notice, "the cut is stated first");
        let tail = &bounded.messages[1..];
        let spent: usize = tail
            .iter()
            .map(crate::compaction::estimated_message_tokens)
            .sum();
        assert!(
            spent <= budget,
            "the verbatim tail fits the budget: {spent}"
        );
        assert_eq!(bounded.included, tail.len());
        assert_eq!(bounded.included + bounded.omitted, 40);
        assert_eq!(
            bounded.reason, REASON_BUDGET,
            "the token budget made the cut, not the contract caps"
        );
        let context = compose(composition(
            &AuthoredLayers::default(),
            Part::Loaded({
                let mut again = Vec::new();
                for index in 0..20 {
                    again.push(message("user", &format!("question {index} {filler}")));
                    again.push(message("assistant", &format!("answer {index} {filler}")));
                }
                again.push(message("user", "the goal"));
                again
            }),
            Part::NotRequested,
        ));
        assert_eq!(
            state_of(&context, history_of).3,
            REASON_BUDGET,
            "the record states the budget reason"
        );
        assert_eq!(
            tail.last().map(|m| m.content.starts_with("answer 19")),
            Some(true),
            "the newest turns are the ones kept"
        );
        assert!(
            !notice.content.contains("reattach_context"),
            "the Do loop has no such tool, so the notice must not name it"
        );
        assert_eq!(
            notice.role, "assistant",
            "a run's history holds no system turn"
        );
    }

    #[test]
    fn the_history_never_exceeds_what_the_executor_accepts() {
        // Forty short turns sit well under any token budget but over the
        // contract's message cap: the executor would refuse the whole run.
        let mut prior: Vec<ChatMessage> = (0..40)
            .flat_map(|i| {
                [
                    message("user", &format!("q{i}")),
                    message("assistant", &format!("a{i}")),
                ]
            })
            .collect();
        prior.push(message("user", "goal"));
        let context = compose(composition(
            &AuthoredLayers::default(),
            Part::Loaded(prior),
            Part::NotRequested,
        ));
        assert_eq!(
            mp_contracts::run_context::validate(Some(&context), false),
            Ok(())
        );
        assert_eq!(context.history.len(), MAX_HISTORY_MESSAGES);
        let (state, included, omitted, reason) = state_of(&context, history_of);
        assert_eq!(state, RunContextState::Truncated);
        assert_eq!((included, omitted), (23, 57));
        assert_eq!(reason, REASON_CAP);

        // One turn larger than the byte cap is left out, not sent.
        let heavy = vec![
            message("user", &"y".repeat(MAX_HISTORY_BYTES + 1)),
            message("assistant", "ok"),
            message("user", "goal"),
        ];
        let bounded = bound_history(heavy, DEFAULT_HISTORY_TOKENS);
        let bytes: usize = bounded.messages.iter().map(|m| m.content.len()).sum();
        assert!(bytes <= MAX_HISTORY_BYTES);
        assert_eq!(bounded.omitted, 1);
    }

    #[test]
    fn memory_timeout_is_unavailable_not_included() {
        // A recall that timed out, failed or had no credential is not "nothing to
        // recall": the record says it could not look, and the prompt gets no block.
        for reason in [REASON_TIMEOUT, REASON_ERROR, REASON_NO_CREDENTIAL] {
            assert_eq!(
                Part::<Vec<String>>::from_plan(Plan::Load, Some(Err(reason))),
                Part::Unavailable(reason)
            );
        }
        let layers = AuthoredLayers::default();
        let context = compose(composition(
            &layers,
            Part::NotRequested,
            Part::from_plan(Plan::Load, Some(Err(REASON_TIMEOUT))),
        ));
        assert_eq!(
            context.memory_block, "",
            "a timed-out recall puts nothing in the prompt"
        );
        assert_eq!(
            state_of(&context, memory_of),
            (
                RunContextState::Unavailable,
                0,
                0,
                REASON_TIMEOUT.to_owned()
            )
        );

        let hit = compose(composition(
            &layers,
            Part::NotRequested,
            Part::from_plan(Plan::Load, Some(Ok(vec!["likes tea".to_owned()]))),
        ));
        assert!(hit.memory_block.contains("likes tea"));
        assert_eq!(
            state_of(&hit, memory_of),
            (RunContextState::Included, 1, 0, String::new())
        );
        let empty = compose(composition(
            &layers,
            Part::NotRequested,
            Part::from_plan(Plan::Load, Some(Ok(Vec::new()))),
        ));
        assert_eq!(
            state_of(&empty, memory_of),
            (RunContextState::Empty, 0, 0, String::new())
        );
    }

    #[test]
    fn an_oversize_memory_block_drops_the_lowest_ranked_entries() {
        let big = "m".repeat(MAX_MEMORY_BYTES / 2);
        let context = compose(composition(
            &AuthoredLayers::default(),
            Part::NotRequested,
            Part::Loaded(vec![big.clone(), big.clone(), big]),
        ));
        assert!(context.memory_block.len() <= MAX_MEMORY_BYTES);
        let (state, included, omitted, reason) = state_of(&context, memory_of);
        assert_eq!(state, RunContextState::Truncated);
        assert_eq!((included, omitted), (1, 2));
        assert_eq!(reason, REASON_CAP);
        assert_eq!(
            mp_contracts::run_context::validate(Some(&context), false),
            Ok(())
        );
    }

    #[test]
    fn oversize_instructions_are_cut_and_stated_not_refused() {
        let layers = AuthoredLayers {
            org: Some("é".repeat(MAX_AUTHORED_INSTRUCTION_BYTES)),
            ..AuthoredLayers::default()
        };
        let context = compose(composition(&layers, Part::NotRequested, Part::NotRequested));
        assert!(context.authored_instructions.len() <= MAX_AUTHORED_INSTRUCTION_BYTES);
        assert_eq!(
            state_of(&context, instructions_of).0,
            RunContextState::Truncated
        );
        assert_eq!(
            mp_contracts::run_context::validate(Some(&context), false),
            Ok(())
        );
    }

    #[test]
    fn trivial_prompt_skips_memory() {
        let all = RunContextMode::Full;
        assert_eq!(
            plan_memory(all, &facts("ok")),
            Plan::Skip(REASON_TRIVIAL_PROMPT)
        );
        assert_eq!(
            plan_memory(all, &facts("/help")),
            Plan::Skip(REASON_TRIVIAL_PROMPT)
        );
        assert_eq!(plan_memory(all, &facts("what is my codename?")), Plan::Load);
        // The history is not affected by the prompt's size.
        assert_eq!(plan_history(all, &facts("ok")), Plan::Load);
    }

    #[test]
    fn space_scoped_run_skips_memory() {
        let space = RunFacts {
            space_scoped: true,
            ..facts("what did the team decide?")
        };
        assert_eq!(
            plan_memory(RunContextMode::Full, &space),
            Plan::Skip(REASON_SPACE_SCOPE)
        );
        let skipped = compose(composition(
            &AuthoredLayers::default(),
            Part::NotRequested,
            Part::from_plan(Plan::Skip(REASON_SPACE_SCOPE), None),
        ));
        assert_eq!(
            state_of(&skipped, memory_of),
            (
                RunContextState::SkippedByPolicy,
                0,
                0,
                REASON_SPACE_SCOPE.to_owned()
            )
        );
        assert_eq!(skipped.memory_block, "");
    }

    #[test]
    fn pinned_route_gets_instructions_only() {
        let pinned = RunFacts {
            pinned_route: true,
            ..facts("what is my codename?")
        };
        for mode in [RunContextMode::Instructions, RunContextMode::Full] {
            assert_ne!(plan_history(mode, &pinned), Plan::Load, "{mode:?}");
            assert_ne!(plan_memory(mode, &pinned), Plan::Load, "{mode:?}");
        }
        assert_eq!(
            plan_history(RunContextMode::Full, &pinned),
            Plan::Skip(REASON_ROUTE_UNCACHED)
        );
        assert_eq!(
            plan_memory(RunContextMode::Full, &pinned),
            Plan::Skip(REASON_ROUTE_UNCACHED)
        );
    }

    #[test]
    fn instructions_mode_reads_neither_history_nor_memory() {
        let plain = facts("what is my codename?");
        assert_eq!(
            plan_history(RunContextMode::Instructions, &plain),
            Plan::NotRequested
        );
        assert_eq!(
            plan_memory(RunContextMode::Instructions, &plain),
            Plan::NotRequested
        );
        let context = compose(composition(
            &layers(),
            Part::NotRequested,
            Part::NotRequested,
        ));
        let status = context.status.expect("status");
        assert!(status.history.is_none() && status.memory.is_none());
    }

    #[test]
    fn a_zero_retention_run_never_carries_history_or_memory() {
        let zdr = RunFacts {
            zdr: true,
            ..facts("what is my codename?")
        };
        assert_eq!(
            plan_history(RunContextMode::Full, &zdr),
            Plan::Skip(REASON_ZDR)
        );
        assert_eq!(
            plan_memory(RunContextMode::Full, &zdr),
            Plan::Skip(REASON_ZDR)
        );
        assert!(
            !zdr.records_status(),
            "a zero-retention run keeps no record"
        );
        let waived = RunFacts {
            waived: true,
            ..facts("what is my codename?")
        };
        assert!(
            !waived.records_status(),
            "a waived credential is refused the write"
        );
        // A waived run is composed like an Ask turn: persistent for its connection.
        assert_eq!(plan_history(RunContextMode::Full, &waived), Plan::Load);
        assert!(facts("goal").records_status());

        let context = compose(composition(
            &layers(),
            Part::from_plan(Plan::Skip(REASON_ZDR), None),
            Part::from_plan(Plan::Skip(REASON_ZDR), None),
        ));
        assert_eq!(
            mp_contracts::run_context::validate(Some(&context), true),
            Ok(()),
            "instructions are allowed under ZDR, history and memory are not"
        );
    }

    #[test]
    fn a_failed_read_becomes_unavailable_and_the_run_still_starts() {
        let context = compose(composition(
            &layers(),
            Part::from_plan(Plan::Load, Some(Err(REASON_TIMEOUT))),
            Part::from_plan(Plan::Load, Some(Err(REASON_NO_CREDENTIAL))),
        ));
        assert!(context.history.is_empty());
        assert_eq!(
            state_of(&context, history_of),
            (
                RunContextState::Unavailable,
                0,
                0,
                REASON_TIMEOUT.to_owned()
            )
        );
        assert_eq!(
            state_of(&context, memory_of),
            (
                RunContextState::Unavailable,
                0,
                0,
                REASON_NO_CREDENTIAL.to_owned()
            )
        );
        assert!(context.authored_instructions.contains("Org rule."));
    }

    #[test]
    fn the_status_is_closed_tokens_and_counts_never_text() {
        let context = compose(composition(
            &layers(),
            Part::Loaded(vec![
                message("user", "SECRET-HISTORY-TEXT"),
                message("assistant", "ok"),
                message("user", "goal"),
            ]),
            Part::Loaded(vec!["SECRET-MEMORY-TEXT".to_owned()]),
        ));
        let json =
            mp_contracts::run_context::status_json(&context.status.expect("status")).to_string();
        for secret in ["SECRET-HISTORY-TEXT", "SECRET-MEMORY-TEXT", "Org rule."] {
            assert!(
                !json.contains(secret),
                "{secret} leaked into the record: {json}"
            );
        }
    }

    #[test]
    fn off_sends_no_field() {
        assert_eq!(RunContextMode::from_lookup(&|_| None), RunContextMode::Off);
        for value in [
            "",
            "off",
            "OFF",
            "FULL",
            "Full",
            "on",
            "true",
            "1",
            "instruction",
            " full",
        ] {
            assert_eq!(
                RunContextMode::parse(value),
                RunContextMode::Off,
                "{value:?}"
            );
        }
        assert_eq!(
            RunContextMode::parse("instructions"),
            RunContextMode::Instructions
        );
        assert_eq!(RunContextMode::parse("full"), RunContextMode::Full);
        assert!(
            RunContextRequest::for_turn(RunContextMode::Off, layers, false, || {
                panic!("off must not even build the memory query")
            }, || panic!("off must not even build the local notice"))
            .is_none(),
            "off builds no request, so no read is made and no field is sent"
        );
        let on = RunContextRequest::for_turn(RunContextMode::Full, layers, false, || {
            "the query".to_owned()
        }, || None)
        .expect("on builds a request");
        assert_eq!(on.memory_query, "the query");
    }

    #[test]
    fn the_history_budget_defaults_and_clamps() {
        let with = |value: Option<&'static str>| {
            history_token_budget_from(&move |_| value.map(str::to_owned))
        };
        assert_eq!(with(None), 8_000);
        assert_eq!(with(Some("")), 8_000);
        assert_eq!(with(Some("lots")), 8_000);
        assert_eq!(with(Some("-5")), 8_000);
        assert_eq!(with(Some("12")), 1_000);
        assert_eq!(with(Some("12000")), 12_000);
        assert_eq!(with(Some("9999999")), 40_000);
    }
}
