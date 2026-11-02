//! Room-safe projection of a live chat turn (build plan WS09-S3).
//!
//! A room turn is the direct tool loop on the owner's invoke SSE. Its text, tool calls, reasoning,
//! grounding and tool output exist only in the owner-keyed `StreamBufferStore`. Other members of the
//! room must see THAT the agent is working and what kind of work it is doing, never the raw frames.
//! This module is the only reader of those frames for the room: it turns a buffered event into a
//! room frame through an allowlist, rebuilding every payload field by field.
//!
//! The rules, in the order they matter:
//!
//! * [`ROOM_EVENT_POLICY`] names every source event. An event that is not in the table is dropped
//!   (default deny); an event in the table with [`Disposition::Drop`] is dropped on purpose. A leak
//!   guard test walks the SSE taxonomy and fails for a name without a row.
//! * Free text never crosses: tool arguments, tool output, step titles and details, plan titles,
//!   steer text, error messages and reasoning are not read at all. A step carries an id, a closed
//!   [`StepClass`] and a closed [`StepState`]. An id passes only if it is a short token.
//! * Assistant text (`chunk`) crosses only when the caller asks for it ([`RoomProjector::new`]
//!   `text_visible`, the `space_run_live_text` decision); otherwise only a character count does.
//! * Offsets are UTF-16 code units (JS `string.length`), so the browser can splice a delta onto its
//!   text without re-deriving a position.
//!
//! The same projector builds the durable `metadata.work` summary the producer saves with the answer
//! ([`work_summary_from_buffer`]). That summary holds no text at all.
//!
//! Nothing here authorizes anything. The member attach (WS09-S4) authorizes through Session Core
//! first; this module only decides what may leave once a caller is authorized.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::stream_buffer::{BufferedEvent, Replay, StreamDone};

/// Version of the room frame vocabulary and of the stored `metadata.work` object.
pub const FRAME_VERSION: u64 = 1;

/// Environment variable of the durable summary writer (`space_run_work_summary`). Off unless an
/// explicit on word; a rollout control, never an authorization input.
pub const WORK_SUMMARY_ENV: &str = "SPACE_RUN_WORK_SUMMARY";

/// The metadata key the summary is stored under.
pub const WORK_KEY: &str = "work";

/// Environment variable of the member live view (`space_run_live_view`). Off unless an explicit on
/// word; a rollout control, never an authorization input.
pub const LIVE_VIEW_ENV: &str = "SPACE_RUN_LIVE_VIEW";

/// Environment variable that lets the live view carry the answer text (`space_run_live_text`). Off
/// means step classes and `text_progress` only.
pub const LIVE_TEXT_ENV: &str = "SPACE_RUN_LIVE_TEXT";

/// Most steps the summary and a snapshot carry.
pub const MAX_STEPS: usize = 64;

/// Most characters (UTF-16 code units) of assistant text a snapshot keeps.
pub const MAX_TEXT_UNITS: u64 = 32_768;

/// Most distinct artifacts a snapshot lists.
pub const MAX_ARTIFACTS: usize = 16;

/// Longest id token a frame carries.
const MAX_ID_LEN: usize = 128;

/// Longest label (agent name) a snapshot carries.
const MAX_LABEL_LEN: usize = 80;

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// Caps the projector enforces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoomLimits {
    pub max_steps: usize,
    pub max_text_units: u64,
    pub max_artifacts: usize,
}

impl RoomLimits {
    pub const DEFAULT: Self = Self {
        max_steps: MAX_STEPS,
        max_text_units: MAX_TEXT_UNITS,
        max_artifacts: MAX_ARTIFACTS,
    };
}

impl Default for RoomLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Pure-value room run policy, read from the environment once at startup (the
/// `AgenticSubscriptionPolicy` pattern) and never read again inside a handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpaceRunPolicy {
    /// `space_run_work_summary`: save `metadata.work` with a room turn's answer.
    pub work_summary: bool,
    /// `space_run_live_view`: serve the member live view route.
    pub live_view: bool,
    /// `space_run_live_text`: the live view carries answer text (requires `live_view`).
    pub live_text: bool,
    pub limits: RoomLimits,
}

impl SpaceRunPolicy {
    /// Everything off: what an unset environment means.
    pub const OFF: Self = Self {
        work_summary: false,
        live_view: false,
        live_text: false,
        limits: RoomLimits::DEFAULT,
    };

    /// The policy the raw value means.
    #[must_use]
    pub fn from_values(
        work_summary: Option<&str>,
        live_view: Option<&str>,
        live_text: Option<&str>,
    ) -> Self {
        let on = mp_contracts::agentic_subscription::switch_value_is_on;
        Self {
            work_summary: on(work_summary),
            live_view: on(live_view),
            live_text: on(live_view) && on(live_text),
            limits: RoomLimits::DEFAULT,
        }
    }

    /// The process environment right now.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var(WORK_SUMMARY_ENV).ok().as_deref(),
            std::env::var(LIVE_VIEW_ENV).ok().as_deref(),
            std::env::var(LIVE_TEXT_ENV).ok().as_deref(),
        )
    }
}

impl Default for SpaceRunPolicy {
    fn default() -> Self {
        Self::OFF
    }
}

/// What the projector does with one source event name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Assistant text delta (`chunk`).
    Text,
    /// A tool or check step (`tool_call`, `tool_result`, `step_update`).
    Step,
    /// The model's structured plan (`plan_proposed`): id and step count only.
    Plan,
    /// A produced artifact: id and kind only.
    Artifact,
    /// Mid-run input delivered to the agent (`queued_input`): a count only.
    Steer,
    /// The stream ended (`stopped`, `error`).
    Terminal,
    /// Not shown to the room.
    Drop,
}

/// Every source event name, and what becomes of it. A name that is not here is dropped, so a new
/// producer event is invisible to the room until a row is added on purpose. Every name in
/// `apps/Model Plane/docs/sse-event-taxonomy.md` has a row (a leak guard test checks it).
pub const ROOM_EVENT_POLICY: &[(&str, Disposition)] = &[
    // Shown, rebuilt field by field.
    ("chunk", Disposition::Text),
    ("tool_call", Disposition::Step),
    ("tool_result", Disposition::Step),
    ("step_update", Disposition::Step),
    ("plan_proposed", Disposition::Plan),
    ("artifact", Disposition::Artifact),
    ("queued_input", Disposition::Steer),
    ("stopped", Disposition::Terminal),
    ("error", Disposition::Terminal),
    // Owner-private content: reasoning, recalled memory, evidence, outputs.
    ("reasoning_delta", Disposition::Drop),
    ("memory_recall", Disposition::Drop),
    ("grounding", Disposition::Drop),
    ("citation", Disposition::Drop),
    ("citations", Disposition::Drop),
    ("search_results", Disposition::Drop),
    ("source_scope_applied", Disposition::Drop),
    ("verification", Disposition::Drop),
    ("attachment", Disposition::Drop),
    ("retrieval_notice", Disposition::Drop),
    // Accounting and presentation of the owner's own turn.
    ("usage", Disposition::Drop),
    ("usage_progress", Disposition::Drop),
    ("turn_timing", Disposition::Drop),
    ("result_receipt", Disposition::Drop),
    ("title", Disposition::Drop),
    ("follow_ups", Disposition::Drop),
    // Lifecycle and transport frames the room does not need.
    ("retry_scheduled", Disposition::Drop),
    ("compaction", Disposition::Drop),
    ("context_change", Disposition::Drop),
    ("run_parked", Disposition::Drop),
    ("run_resumed", Disposition::Drop),
    ("awaiting_approval", Disposition::Drop),
    ("user_question", Disposition::Drop),
    ("connected", Disposition::Drop),
    ("done", Disposition::Drop),
];

/// What the projector does with `name`. Default deny.
#[must_use]
pub fn disposition(name: &str) -> Disposition {
    ROOM_EVENT_POLICY
        .iter()
        .find(|(known, _)| *known == name)
        .map_or(Disposition::Drop, |(_, disposition)| *disposition)
}

// ---------------------------------------------------------------------------
// Step vocabulary
// ---------------------------------------------------------------------------

/// What kind of work a step is. A closed set; `browser` is reserved for WS20 and not produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepClass {
    SearchKnowledge,
    Web,
    Code,
    Artifact,
    Lookup,
    Effect,
    Memory,
    Delegate,
    Plan,
    Check,
    Tool,
}

impl StepClass {
    /// Every class, in the order the golden lists them.
    pub const ALL: [StepClass; 11] = [
        StepClass::SearchKnowledge,
        StepClass::Web,
        StepClass::Code,
        StepClass::Artifact,
        StepClass::Lookup,
        StepClass::Effect,
        StepClass::Memory,
        StepClass::Delegate,
        StepClass::Plan,
        StepClass::Check,
        StepClass::Tool,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            StepClass::SearchKnowledge => "search_knowledge",
            StepClass::Web => "web",
            StepClass::Code => "code",
            StepClass::Artifact => "artifact",
            StepClass::Lookup => "lookup",
            StepClass::Effect => "effect",
            StepClass::Memory => "memory",
            StepClass::Delegate => "delegate",
            StepClass::Plan => "plan",
            StepClass::Check => "check",
            StepClass::Tool => "tool",
        }
    }

    /// The class for a stored word, or `None` for a word outside the closed set.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.as_str() == word)
    }

    /// The class of a tool call by its name. `None` means an internal tool the room never sees
    /// (the call and its result both emit nothing). A name that is not known is `Tool`.
    #[must_use]
    pub fn for_tool(name: &str) -> Option<Self> {
        let name = name.trim();
        if let Some((_, class)) = BUILTIN_TOOL_CLASSES.iter().find(|(known, _)| *known == name) {
            return *class;
        }
        let lower = name.to_ascii_lowercase();
        let starts = |prefix: &str| lower.starts_with(prefix);
        Some(if starts("plan.") || starts("plan_") {
            StepClass::Plan
        } else if starts("memory.") || starts("memory_") {
            StepClass::Memory
        } else if starts("delegate") || starts("subagent") || starts("agent.") {
            StepClass::Delegate
        } else if starts("effect.") {
            StepClass::Effect
        } else if starts("knowledge_") {
            StepClass::SearchKnowledge
        } else {
            StepClass::Tool
        })
    }
}

/// The class of every tool `tool_loop::builtin_tool_defs` advertises. `None` is an internal tool.
/// A test checks that every advertised name has a row.
pub const BUILTIN_TOOL_CLASSES: &[(&str, Option<StepClass>)] = &[
    ("count_words", Some(StepClass::Check)),
    ("reattach_context", None),
    ("reattach_skill", None),
    ("skill_file", Some(StepClass::Lookup)),
    ("web_search", Some(StepClass::Web)),
    ("fetch_url", Some(StepClass::Web)),
    ("get_weather", Some(StepClass::Lookup)),
    ("get_statistics", Some(StepClass::Lookup)),
    ("code_interpreter", Some(StepClass::Code)),
    ("create_artifact", Some(StepClass::Artifact)),
    ("read_artifact", Some(StepClass::Artifact)),
    ("update_artifact", Some(StepClass::Artifact)),
    ("inbox_search", Some(StepClass::Lookup)),
    ("inbox_get_conversation", Some(StepClass::Lookup)),
    ("result_query", Some(StepClass::Lookup)),
    ("knowledge_search", Some(StepClass::SearchKnowledge)),
    ("knowledge_graph_search", Some(StepClass::SearchKnowledge)),
    ("knowledge_wiki_search", Some(StepClass::SearchKnowledge)),
    ("knowledge_contradictions", Some(StepClass::SearchKnowledge)),
    ("knowledge_list_documents", Some(StepClass::SearchKnowledge)),
    ("shipping_get_quotes", Some(StepClass::Lookup)),
    ("insights_overview", Some(StepClass::Lookup)),
    ("social_list_accounts", Some(StepClass::Lookup)),
    ("social_list_posts", Some(StepClass::Lookup)),
    ("social_list_campaigns", Some(StepClass::Lookup)),
];

/// Where a step is. Closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepState {
    Running,
    Ok,
    Error,
    Denied,
}

impl StepState {
    pub const ALL: [StepState; 4] = [
        StepState::Running,
        StepState::Ok,
        StepState::Error,
        StepState::Denied,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            StepState::Running => "running",
            StepState::Ok => "ok",
            StepState::Error => "error",
            StepState::Denied => "denied",
        }
    }

    /// The state a tool result's `status` word means. Only a plain success is `ok`; any word that
    /// is not understood is an error, never a success.
    fn from_result_status(status: &str) -> Self {
        match status.trim().to_ascii_lowercase().as_str() {
            "ok" | "done" | "success" | "completed" | "complete" => StepState::Ok,
            "denied" | "blocked" | "forbidden" | "refused" => StepState::Denied,
            "running" | "pending" | "started" => StepState::Running,
            _ => StepState::Error,
        }
    }
}

/// One step as the room sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomStep {
    pub id: String,
    pub class: StepClass,
    pub state: StepState,
    /// Elapsed milliseconds when the caller knows it. A buffer replay has no timestamps, so a
    /// projection built from one leaves it empty.
    pub ms: Option<u64>,
}

impl RoomStep {
    fn to_json(&self) -> Value {
        let mut step = Map::new();
        step.insert("id".to_owned(), json!(self.id));
        step.insert("class".to_owned(), json!(self.class.as_str()));
        step.insert("state".to_owned(), json!(self.state.as_str()));
        if let Some(ms) = self.ms {
            step.insert("ms".to_owned(), json!(ms));
        }
        Value::Object(step)
    }
}

// ---------------------------------------------------------------------------
// Terminal vocabulary
// ---------------------------------------------------------------------------

/// How the turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Failed,
    Stopped,
}

impl Outcome {
    pub const ALL: [Outcome; 3] = [Outcome::Completed, Outcome::Failed, Outcome::Stopped];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Completed => "completed",
            Outcome::Failed => "failed",
            Outcome::Stopped => "stopped",
        }
    }
}

/// Why a turn failed, in a closed set. Never the error text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    ModelUnavailable,
    Budget,
    Policy,
    Internal,
    Unknown,
}

impl Reason {
    pub const ALL: [Reason; 5] = [
        Reason::ModelUnavailable,
        Reason::Budget,
        Reason::Policy,
        Reason::Internal,
        Reason::Unknown,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::ModelUnavailable => "model_unavailable",
            Reason::Budget => "budget",
            Reason::Policy => "policy",
            Reason::Internal => "internal",
            Reason::Unknown => "unknown",
        }
    }

    /// The reason a machine error code means. Only the code is read (a stable token), never the
    /// message.
    #[must_use]
    pub fn from_error_code(code: &str) -> Self {
        let code = code.trim().to_ascii_lowercase();
        let has = |needle: &str| code.contains(needle);
        if has("budget") || has("quota") || has("ceiling") || has("limit") {
            Reason::Budget
        } else if has("zdr") || has("policy") || has("moderation") || has("refus") || has("forbidden") {
            Reason::Policy
        } else if has("model") || has("provider") || has("inference") || has("upstream") || has("subscription") {
            Reason::ModelUnavailable
        } else if has("internal") || has("session") || has("liveness") || has("persist") {
            Reason::Internal
        } else {
            Reason::Unknown
        }
    }
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

/// What a snapshot says about the run that the projector cannot know from the buffer. The caller
/// (the attach handler, after Session Core authorized it) supplies it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotHead {
    pub run_id: String,
    pub thread_id: String,
    pub started_by: String,
    pub agent_name: String,
    pub phase: String,
    pub started_at: i64,
    pub expires_at: i64,
}

/// One room frame. `event_name` is the SSE `event:` name; `to_json` is the `data:` payload.
#[derive(Clone, Debug, PartialEq)]
pub enum RoomFrame {
    /// The first frame of a stream: everything the projector knows.
    Snapshot(Value),
    Text { offset: u64, delta: String },
    TextProgress { chars: u64 },
    Step(RoomStep),
    Steer { count: u32 },
    Plan { plan_id: String, steps: u32 },
    Artifact { id: String, kind: String },
    Finished { outcome: Outcome, reason: Option<Reason> },
    ReauthRequired,
    Error { code: String, message: String, retryable: bool, feature: Option<String> },
}

impl RoomFrame {
    /// The SSE `event:` name.
    #[must_use]
    pub fn event_name(&self) -> &'static str {
        match self {
            RoomFrame::Snapshot(_) => "snapshot",
            RoomFrame::Text { .. } => "text",
            RoomFrame::TextProgress { .. } => "text_progress",
            RoomFrame::Step(_) => "step",
            RoomFrame::Steer { .. } => "steer",
            RoomFrame::Plan { .. } => "plan",
            RoomFrame::Artifact { .. } => "artifact",
            RoomFrame::Finished { .. } => "finished",
            RoomFrame::ReauthRequired => "reauth_required",
            RoomFrame::Error { .. } => "error",
        }
    }

    /// The SSE `data:` payload.
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            RoomFrame::Snapshot(value) => value.clone(),
            RoomFrame::Text { offset, delta } => json!({ "offset": offset, "delta": delta }),
            RoomFrame::TextProgress { chars } => json!({ "chars": chars }),
            RoomFrame::Step(step) => step.to_json(),
            RoomFrame::Steer { count } => json!({ "count": count }),
            RoomFrame::Plan { plan_id, steps } => json!({ "plan_id": plan_id, "steps": steps }),
            RoomFrame::Artifact { id, kind } => json!({ "id": id, "kind": kind }),
            RoomFrame::Finished { outcome, reason } => json!({
                "outcome": outcome.as_str(),
                "reason": reason.map(Reason::as_str),
            }),
            RoomFrame::ReauthRequired => json!({}),
            RoomFrame::Error { code, message, retryable, feature } => {
                let mut error = Map::new();
                error.insert("code".to_owned(), json!(code));
                error.insert("message".to_owned(), json!(message));
                error.insert("retryable".to_owned(), json!(retryable));
                if let Some(feature) = feature {
                    error.insert("feature".to_owned(), json!(feature));
                }
                Value::Object(error)
            }
        }
    }
}

/// A frame and the source sequence number it is delivered under (the SSE `id:`).
#[derive(Clone, Debug, PartialEq)]
pub struct Emitted {
    pub seq: u64,
    pub frame: RoomFrame,
}

// ---------------------------------------------------------------------------
// Projector
// ---------------------------------------------------------------------------

/// Folds buffered source events into room state and room frames. Pure: no I/O, no clock.
pub struct RoomProjector {
    limits: RoomLimits,
    text_visible: bool,
    /// The kept window of assistant text (empty while `text_visible` is false).
    text: String,
    /// UTF-16 length of everything ever appended, and of the kept window.
    text_total_units: u64,
    text_kept_units: u64,
    text_truncated: bool,
    steps: Vec<RoomStep>,
    step_index: HashMap<String, usize>,
    /// Call ids of internal tools, whose results must also emit nothing.
    ignored_calls: HashSet<String>,
    steer: u32,
    plan: Option<(String, u32)>,
    artifacts: Vec<(String, String)>,
    finished: Option<(Outcome, Option<Reason>)>,
}

impl RoomProjector {
    /// `text_visible` is the `space_run_live_text` decision: with it false the projector keeps no
    /// text at all and reports only how much there is.
    #[must_use]
    pub fn new(limits: RoomLimits, text_visible: bool) -> Self {
        Self {
            limits,
            text_visible,
            text: String::new(),
            text_total_units: 0,
            text_kept_units: 0,
            text_truncated: false,
            steps: Vec::new(),
            step_index: HashMap::new(),
            ignored_calls: HashSet::new(),
            steer: 0,
            plan: None,
            artifacts: Vec::new(),
            finished: None,
        }
    }

    /// Whether the turn has ended (a terminal source frame or `done` was applied).
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished.is_some()
    }

    /// Fold a batch of source events in order and return the frames they produce. Consecutive text
    /// deltas coalesce into one `text` frame; a batch that only grew hidden text yields one
    /// `text_progress`. The caller delivers each frame under its `seq`.
    pub fn apply_batch<'a>(
        &mut self,
        events: impl IntoIterator<Item = &'a BufferedEvent>,
    ) -> Vec<Emitted> {
        let mut out: Vec<Emitted> = Vec::new();
        // (offset of the first pending delta, pending text, seq of the last delta)
        let mut pending: Option<(u64, String, u64)> = None;
        let mut progress_seq: Option<u64> = None;

        for event in events {
            if self.finished.is_some() {
                break;
            }
            let seq = event.seq;
            let disposition = if event.is_legacy_text() {
                Disposition::Text
            } else {
                disposition(&event.event)
            };
            if disposition == Disposition::Text {
                let delta = if event.is_legacy_text() {
                    event.delta.clone()
                } else {
                    chunk_delta(&event.data)
                };
                if delta.is_empty() {
                    continue;
                }
                let offset = self.append_text(&delta);
                if self.text_visible {
                    match pending.as_mut() {
                        Some((_, text, last)) => {
                            text.push_str(&delta);
                            *last = seq;
                        }
                        None => pending = Some((offset, delta, seq)),
                    }
                } else {
                    progress_seq = Some(seq);
                }
                continue;
            }
            // Any other frame ends the text run so order is kept.
            Self::flush_text(&mut pending, &mut out);
            if let Some(seq) = progress_seq.take() {
                out.push(Emitted { seq, frame: RoomFrame::TextProgress { chars: self.text_total_units } });
            }
            match disposition {
                Disposition::Step => self.apply_step(event, &mut out),
                Disposition::Plan => self.apply_plan(event, &mut out),
                Disposition::Artifact => self.apply_artifact(event, &mut out),
                Disposition::Steer => self.apply_steer(event, &mut out),
                Disposition::Terminal => self.apply_terminal(event, &mut out),
                Disposition::Text | Disposition::Drop => {}
            }
        }
        Self::flush_text(&mut pending, &mut out);
        if let Some(seq) = progress_seq {
            out.push(Emitted { seq, frame: RoomFrame::TextProgress { chars: self.text_total_units } });
        }
        out
    }

    /// Fold a whole replay: its frames, then its terminal record. A `done` record finishes the run
    /// as completed. An `ended` record with no terminal frame seen finishes it as failed with an
    /// unknown reason, never as completed.
    pub fn apply_replay(&mut self, replay: &Replay) -> Vec<Emitted> {
        let mut out = self.apply_batch(replay.deltas.iter());
        if self.finished.is_none() {
            if let Some(done) = replay.done.as_ref() {
                out.extend(self.apply_done(done));
            } else if let Some(ended) = replay.ended.as_ref() {
                self.finished = Some((Outcome::Failed, Some(Reason::Unknown)));
                out.push(Emitted {
                    seq: ended.seq,
                    frame: RoomFrame::Finished { outcome: Outcome::Failed, reason: Some(Reason::Unknown) },
                });
            }
        }
        out
    }

    /// The terminal `done` record: the run completed.
    pub fn apply_done(&mut self, done: &StreamDone) -> Vec<Emitted> {
        if self.finished.is_some() {
            return Vec::new();
        }
        self.finished = Some((Outcome::Completed, None));
        vec![Emitted {
            seq: done.seq,
            frame: RoomFrame::Finished { outcome: Outcome::Completed, reason: None },
        }]
    }

    fn flush_text(pending: &mut Option<(u64, String, u64)>, out: &mut Vec<Emitted>) {
        if let Some((offset, delta, seq)) = pending.take() {
            out.push(Emitted { seq, frame: RoomFrame::Text { offset, delta } });
        }
    }

    /// Append to the text accounting and return the UTF-16 offset the delta starts at.
    fn append_text(&mut self, delta: &str) -> u64 {
        let before = self.text_total_units;
        let units = utf16_len(delta);
        self.text_total_units = self.text_total_units.saturating_add(units);
        if self.text_visible {
            self.text.push_str(delta);
            self.text_kept_units = self.text_kept_units.saturating_add(units);
            self.trim_text();
        }
        before
    }

    /// Keep the newest `max_text_units` of text, cutting on a character boundary.
    fn trim_text(&mut self) {
        if self.text_kept_units <= self.limits.max_text_units {
            return;
        }
        let mut excess = self.text_kept_units - self.limits.max_text_units;
        let mut cut = 0usize;
        for (index, ch) in self.text.char_indices() {
            if excess == 0 {
                break;
            }
            cut = index + ch.len_utf8();
            excess = excess.saturating_sub(ch.len_utf16() as u64);
        }
        self.text.drain(..cut);
        self.text_kept_units = utf16_len(&self.text);
        self.text_truncated = true;
    }

    fn upsert_step(&mut self, id: &str, class: StepClass, state: StepState, seq: u64, out: &mut Vec<Emitted>) {
        let step = match self.step_index.get(id).copied() {
            Some(index) => {
                let step = &mut self.steps[index];
                step.state = state;
                step.clone()
            }
            None => {
                if self.steps.len() >= self.limits.max_steps {
                    return;
                }
                let step = RoomStep { id: id.to_owned(), class, state, ms: None };
                self.step_index.insert(id.to_owned(), self.steps.len());
                self.steps.push(step.clone());
                step
            }
        };
        out.push(Emitted { seq, frame: RoomFrame::Step(step) });
    }

    fn apply_step(&mut self, event: &BufferedEvent, out: &mut Vec<Emitted>) {
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            return;
        };
        let Some(id) = data.get("id").and_then(Value::as_str).and_then(safe_token) else {
            return;
        };
        match event.event.as_str() {
            "tool_call" => {
                let name = data.get("name").and_then(Value::as_str).unwrap_or_default();
                match StepClass::for_tool(name) {
                    Some(class) => self.upsert_step(&id, class, StepState::Running, event.seq, out),
                    None => {
                        self.ignored_calls.insert(id);
                    }
                }
            }
            "tool_result" => {
                if self.ignored_calls.contains(&id) {
                    return;
                }
                // A result for a call that was never seen (or fell past the cap) has no class and
                // is not guessed at.
                let Some(index) = self.step_index.get(&id).copied() else {
                    return;
                };
                let class = self.steps[index].class;
                let status = data.get("status").and_then(Value::as_str).unwrap_or_default();
                let mut state = StepState::from_result_status(status);
                if state == StepState::Ok && data.get("error").is_some_and(|error| !error.is_null()) {
                    state = StepState::Error;
                }
                self.upsert_step(&id, class, state, event.seq, out);
            }
            "step_update" => {
                // Only the post-tool source check is a step of its own; every other step_update is
                // free-text narration of work the tool frames already describe.
                if !id.ends_with(":source-check") {
                    return;
                }
                let status = data.get("status").and_then(Value::as_str).unwrap_or_default();
                let state = StepState::from_result_status(status);
                self.upsert_step(&id, StepClass::Check, state, event.seq, out);
            }
            _ => {}
        }
    }

    fn apply_plan(&mut self, event: &BufferedEvent, out: &mut Vec<Emitted>) {
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            return;
        };
        let Some(plan_id) = data.get("plan_id").and_then(Value::as_str).and_then(safe_token) else {
            return;
        };
        let steps = data
            .get("steps")
            .and_then(Value::as_array)
            .map_or(0, |steps| u32::try_from(steps.len()).unwrap_or(u32::MAX));
        self.plan = Some((plan_id.clone(), steps));
        out.push(Emitted { seq: event.seq, frame: RoomFrame::Plan { plan_id, steps } });
    }

    fn apply_artifact(&mut self, event: &BufferedEvent, out: &mut Vec<Emitted>) {
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            return;
        };
        let Some(id) = data.get("id").and_then(Value::as_str).and_then(safe_token) else {
            return;
        };
        let kind = data
            .get("kind")
            .and_then(Value::as_str)
            .and_then(|kind| safe_token_max(kind, 32))
            .unwrap_or_else(|| "unknown".to_owned());
        // A new version of an artifact already listed is not a new artifact.
        if self.artifacts.iter().any(|(known, _)| *known == id)
            || self.artifacts.len() >= self.limits.max_artifacts
        {
            return;
        }
        self.artifacts.push((id.clone(), kind.clone()));
        out.push(Emitted { seq: event.seq, frame: RoomFrame::Artifact { id, kind } });
    }

    fn apply_steer(&mut self, event: &BufferedEvent, out: &mut Vec<Emitted>) {
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            return;
        };
        // Only the number of delivered messages is read. The text of a steer never leaves the owner.
        let delivered = data
            .get("messages")
            .and_then(Value::as_array)
            .map_or(0, |messages| u32::try_from(messages.len()).unwrap_or(u32::MAX));
        if delivered == 0 {
            return;
        }
        self.steer = self.steer.saturating_add(delivered);
        out.push(Emitted { seq: event.seq, frame: RoomFrame::Steer { count: self.steer } });
    }

    fn apply_terminal(&mut self, event: &BufferedEvent, out: &mut Vec<Emitted>) {
        let (outcome, reason) = match event.event.as_str() {
            "stopped" => (Outcome::Stopped, None),
            "error" => {
                // The code is a machine token; the message is never read.
                let code = serde_json::from_str::<Value>(&event.data)
                    .ok()
                    .and_then(|data| data.get("code").and_then(Value::as_str).map(str::to_owned))
                    .unwrap_or_default();
                (Outcome::Failed, Some(Reason::from_error_code(&code)))
            }
            _ => return,
        };
        self.finished = Some((outcome, reason));
        out.push(Emitted { seq: event.seq, frame: RoomFrame::Finished { outcome, reason } });
    }

    /// The `snapshot` frame: the state of the run as folded so far.
    #[must_use]
    pub fn snapshot(&self, head: &SnapshotHead) -> RoomFrame {
        let text = if self.text_visible {
            json!({
                "offset": self.text_total_units - self.text_kept_units,
                "value": self.text,
                "truncated": self.text_truncated,
                "visible": true,
            })
        } else {
            // Hidden text: the offset is how much there is, the value is empty.
            json!({
                "offset": self.text_total_units,
                "value": "",
                "truncated": false,
                "visible": false,
            })
        };
        let plan = self
            .plan
            .as_ref()
            .map_or(Value::Null, |(plan_id, steps)| json!({ "plan_id": plan_id, "steps": steps }));
        RoomFrame::Snapshot(json!({
            "v": FRAME_VERSION,
            "run_id": head.run_id,
            "thread_id": head.thread_id,
            "started_by": head.started_by,
            "agent_name": clean_label(&head.agent_name),
            "phase": head.phase,
            "started_at": head.started_at,
            "text": text,
            "steps": self.steps.iter().map(RoomStep::to_json).collect::<Vec<_>>(),
            "steer": self.steer,
            "plan": plan,
            "artifacts": self
                .artifacts
                .iter()
                .map(|(id, kind)| json!({ "id": id, "kind": kind }))
                .collect::<Vec<_>>(),
            "expires_at": head.expires_at,
        }))
    }

    /// The durable summary: steps and the steer count, no text. `None` when the turn did no
    /// room-visible work, so an answer with no steps keeps the metadata it had.
    ///
    /// A step still `running` when the summary is written never reported a result, and is stored as
    /// `error`: a stored summary never claims a success nobody saw.
    #[must_use]
    pub fn work_summary(&self) -> Option<Value> {
        if self.steps.is_empty() && self.steer == 0 {
            return None;
        }
        let steps: Vec<Value> = self
            .steps
            .iter()
            .map(|step| {
                let mut step = step.clone();
                if step.state == StepState::Running {
                    step.state = StepState::Error;
                }
                step.to_json()
            })
            .collect();
        Some(json!({ "v": FRAME_VERSION, "steps": steps, "steer": self.steer }))
    }
}

/// Build the durable `metadata.work` object from a producer's own buffer. Text is never kept.
#[must_use]
pub fn work_summary_from_buffer(replay: &Replay, limits: RoomLimits) -> Option<Value> {
    if !replay.found {
        return None;
    }
    let mut projector = RoomProjector::new(limits, false);
    let _ = projector.apply_replay(replay);
    projector.work_summary()
}

/// Rebuild a stored `work` value into the closed shape, or `None` when it is not that shape.
/// A transcript read for a room runs a stored value through this, so a malformed or tampered
/// value is dropped rather than passed along.
#[must_use]
pub fn validate_work_value(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    if object.get("v").and_then(Value::as_u64) != Some(FRAME_VERSION) {
        return None;
    }
    let steer = object
        .get("steer")
        .and_then(Value::as_u64)
        .and_then(|steer| u32::try_from(steer).ok())
        .unwrap_or(0);
    let mut steps = Vec::new();
    for step in object.get("steps")?.as_array()?.iter().take(MAX_STEPS) {
        let step = step.as_object()?;
        let id = step.get("id").and_then(Value::as_str).and_then(safe_token)?;
        let class = step.get("class").and_then(Value::as_str).and_then(StepClass::parse)?;
        let state = match step.get("state").and_then(Value::as_str)? {
            "ok" => StepState::Ok,
            "error" => StepState::Error,
            "denied" => StepState::Denied,
            _ => return None,
        };
        steps.push(
            RoomStep { id, class, state, ms: step.get("ms").and_then(Value::as_u64) }.to_json(),
        );
    }
    Some(json!({ "v": FRAME_VERSION, "steps": steps, "steer": steer }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// UTF-16 code units in `text` (JS `string.length`).
#[must_use]
pub fn utf16_len(text: &str) -> u64 {
    text.chars().map(|ch| ch.len_utf16() as u64).sum()
}

/// The `delta` of a `chunk` payload, or empty.
fn chunk_delta(data: &str) -> String {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| value.get("delta").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}

fn safe_token(raw: &str) -> Option<String> {
    safe_token_max(raw, MAX_ID_LEN)
}

/// `raw` when it is a short token of letters, digits and `_ . : -`. Anything else (spaces, control
/// characters, a sentence) is not an id and is refused rather than cleaned.
fn safe_token_max(raw: &str, max: usize) -> Option<String> {
    let raw = raw.trim();
    let ok = !raw.is_empty()
        && raw.len() <= max
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-'));
    ok.then(|| raw.to_owned())
}

/// A display label with control characters removed and a length cap.
fn clean_label(raw: &str) -> String {
    raw.chars()
        .filter(|ch| !ch.is_control())
        .take(MAX_LABEL_LEN)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(seq: u64, name: &str, data: Value) -> BufferedEvent {
        BufferedEvent { seq, event: name.to_owned(), data: data.to_string(), delta: String::new() }
    }

    #[test]
    fn an_unnamed_event_is_dropped() {
        assert_eq!(disposition("brand_new_event"), Disposition::Drop);
        assert_eq!(disposition("reasoning_delta"), Disposition::Drop);
        assert_eq!(disposition("chunk"), Disposition::Text);
    }

    #[test]
    fn tool_text_and_secrets_never_reach_a_frame() {
        let mut projector = RoomProjector::new(RoomLimits::DEFAULT, true);
        let events = vec![
            event(0, "tool_call", json!({"id":"c1","name":"web_search","args":{"q":"SECRET-ARG"}})),
            event(1, "tool_result", json!({"id":"c1","status":"ok","output":"SECRET-OUT","error":null})),
            event(2, "reasoning_delta", json!({"delta":"SECRET-THOUGHT"})),
            event(3, "queued_input", json!({"messages":["SECRET-STEER"]})),
        ];
        let frames = projector.apply_batch(events.iter());
        let rendered = frames.iter().map(|e| e.frame.to_json().to_string()).collect::<String>();
        assert!(!rendered.contains("SECRET"));
        assert_eq!(frames.len(), 3);
    }

    #[test]
    fn offsets_count_utf16_units() {
        let mut projector = RoomProjector::new(RoomLimits::DEFAULT, true);
        let first = event(0, "chunk", json!({"delta":"Blåbær 😀"}));
        let second = event(1, "chunk", json!({"delta":"x"}));
        let frames = projector.apply_batch([&first, &second]);
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].frame,
            RoomFrame::Text { offset: 0, delta: "Blåbær 😀x".to_owned() }
        );
        assert_eq!(projector.text_total_units, 10);
    }
}
