//! Per-turn phase timing for a streamed chat turn (Q14).
//!
//! One monotonic clock per turn. It starts where `sse.rs` mints the request id
//! and moves into the stream task with everything else, so admission, session
//! preparation and context assembly are on the same origin as the tool rounds,
//! the draft checks and the durable completion that follow.
//!
//! The clock keeps a bounded list of phase spans, a few named marks and the
//! interval of every provider call, and reports them three ways:
//!
//! - the `turn_timing` SSE event (in the `usage` family), once, just before the
//!   turn's terminal `done`, `error` or `stopped` frame;
//! - a summary without the span list on `USAGE_ENVELOPE`;
//! - one `turn timing` info line when the last handle drops. That is every
//!   exit: a failure, a timeout, a Stop, and a client that disconnects before
//!   the stream task even started, which drops the handler and with it the
//!   clock. A turn that never declared an outcome is logged as `cancelled`.
//!
//! Only identifiers, milliseconds, counts and offered tool names are recorded:
//! never prompt text, message content, tool arguments or tool results. The
//! same record is therefore safe for ZDR and temporary chats. The draft-size
//! record ([`Draft`]) is character counts and one boolean, nothing else.
//!
//! `provider_ms` is the union of the gateway-observed provider-call intervals,
//! so parallel review batches are counted once, and `orchestration_ms` is the
//! rest of `server_ms`. Per call, the gateway interval minus inference-core's
//! `handler_ms` is transport, and `handler_ms` minus its `provider_ms` is
//! inference-core's own overhead.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mp_contracts::model_plane::v1::{InferResponse, ProviderTiming};
use serde::Serialize;
use tokio::time::Instant;

/// Most phase spans one turn keeps. Later spans are only counted, in
/// `phases_dropped`, so a runaway tool loop cannot grow the event unbounded.
pub const MAX_PHASES: usize = 64;

/// Most tool names one span carries.
const MAX_SPAN_TOOLS: usize = 16;

/// The named stages of a turn.
///
/// Spans may nest: a draft check's `local_validation`, `source_review`,
/// `review_batch` and `repair` spans all fall inside the tool phase, and
/// parallel review batches overlap each other. Consumers must not sum spans;
/// `provider_ms` and `orchestration_ms` are the non-overlapping totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Request id minted until session preparation: capability, org-limit,
    /// budget and idempotency checks.
    Admission,
    /// Session Core managed-run preparation and the initial liveness heartbeat.
    SessionPrepare,
    /// Context assembly, history, memory, grounding, skills and prompt
    /// building, until the stream task starts.
    Context,
    /// A tool round that did not propose a document write.
    ToolDecision,
    /// A tool round that proposed `create_artifact` or `update_artifact`: the
    /// author call that writes the whole draft into the tool arguments.
    Generation,
    /// Gateway-local work inside a draft check while no provider call of that
    /// check is in flight: the local document checks, applying reviewer edits
    /// and building the next review or repair.
    LocalValidation,
    /// One single-call source-review inference.
    SourceReview,
    /// One of a split review's parallel parts.
    ReviewBatch,
    /// A private repair inference, for a draft or a word-range answer.
    Repair,
    /// One round's tool dispatch: audit reserve, run and audit finalize.
    Dispatch,
    /// The answer call: the streamed answer, or a word-range answer candidate.
    AnswerStream,
    /// The low-confidence verification pass.
    Verification,
    /// Waiting for the thread's artifact commit lock.
    CommitLock,
    /// Appending the assistant message to Session Core.
    DurableAppend,
    /// Session Core's terminal acknowledgement, the durable completion point.
    TerminalAck,
    /// Publishing the artifacts, the receipt, the usage envelope and the usage
    /// event after durable completion.
    Publish,
    /// Thread title inference.
    Title,
    /// Follow-up suggestion inference.
    FollowUps,
}

/// Named points in a turn, each kept at its first occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// The turn's attachments were parsed into a source context.
    SourceReady,
    /// The `connected` frame was handed to the response.
    Connected,
    /// The first non-empty content or reasoning delta of a streamed answer.
    FirstToken,
    /// The first author call that proposed a document write returned.
    FirstDraft,
    /// The first draft check accepted its draft.
    FirstValidated,
    /// Session Core acknowledged the completed turn.
    Durable,
}

/// How a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Failed,
    Cancelled,
    Timeout,
}

impl Outcome {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
        }
    }

    /// The outcome a failure code stands for: an elapsed deadline is a
    /// timeout, every other failure is a failure.
    #[must_use]
    pub fn of_failure(code: &str) -> Self {
        if code == crate::result_validation::VALIDATION_TIMEOUT {
            Self::Timeout
        } else {
            Self::Failed
        }
    }
}

/// The named marks, in milliseconds from the turn origin. Absent until reached.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Marks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ready_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_token_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_draft_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_validated_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durable_ms: Option<u64>,
}

impl Marks {
    fn slot(&mut self, mark: Mark) -> &mut Option<u64> {
        match mark {
            Mark::SourceReady => &mut self.source_ready_ms,
            Mark::Connected => &mut self.connected_ms,
            Mark::FirstToken => &mut self.first_token_ms,
            Mark::FirstDraft => &mut self.first_draft_ms,
            Mark::FirstValidated => &mut self.first_validated_ms,
            Mark::Durable => &mut self.durable_ms,
        }
    }
}

/// What the turn did, counted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    /// Tool-loop rounds that called the model.
    pub model_rounds: u32,
    /// Every provider call the gateway timed, including abandoned ones.
    pub provider_calls: u32,
    /// Every candidate actually sent to source review, one entry per
    /// `check_artifact` attempt that reached the reviewer — a re-review of a
    /// byte-identical candidate (a rejected PRESERVED segment's edit
    /// `Preservation::apply` reverted) counts as its own pass, never folded
    /// into an earlier one by the candidate hash alone (owner ruling
    /// 2026-09-27, truthful counts). A protocol retry of the SAME attempt is
    /// not a separate pass; see `protocol_retries`.
    pub review_passes: u32,
    /// Every batch assignment actually sent, across every pass above,
    /// distinct in the same way.
    pub review_batches: u32,
    /// Private repairs, of drafts and of word-range answers.
    pub repairs: u32,
    /// Reviews of a candidate that the reviewer's own proposed edits produced.
    pub reviewer_edits: u32,
    /// Source-review calls (whole or one split batch) retried after a
    /// protocol rejection: the model's own report did not match the required
    /// schema, or did not cover every assigned segment. Invisible in
    /// `review_passes`/`review_batches` alone, which count distinct
    /// candidates/batches, not the attempts spent reaching an accepted one.
    pub protocol_retries: u32,
}

/// How large the turn's document writes were: numbers and one boolean, never
/// the text. It lets a series of turns be analysed for how much of a revised
/// document the model rewrites, and whether it adopted a shorter draft when
/// it was asked for one (`LatencyLevers::dated_post_section_only`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Draft {
    /// `create_artifact` and `update_artifact` calls the model emitted, in
    /// every round of the turn, counted before any gateway check, splice or
    /// drop.
    pub model_writes: u32,
    /// Characters in the raw `content` argument of those calls, as the model
    /// emitted them and measured before any preserved section was spliced in.
    /// A call cut off mid-argument (the output ceiling) has no parseable
    /// argument: the length of its raw arguments text stands in for it.
    pub model_write_chars: u64,
    /// Characters of the document the turn stored, after every check and
    /// splice: the last document write the turn dispatched. 0 when none was.
    pub stored_chars: u64,
    /// The model answered a dated-post revision with that post's section
    /// alone, and the gateway spliced it into the accepted document
    /// (`Preservation::is_section_fragment`).
    pub fragment_spliced: bool,
}

impl Draft {
    /// Nothing was recorded: the turn wrote no document. An empty draft is
    /// left out of the report, so a turn without a document writes the record
    /// it always did.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Inference-core's report about one call, as the event carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderSummary {
    pub handler_ms: u64,
    pub provider_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_token_ms: Option<u64>,
    pub attempts: u32,
    pub response_cache_hit: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub upstream_ms: BTreeMap<String, u64>,
}

impl From<&ProviderTiming> for ProviderSummary {
    fn from(timing: &ProviderTiming) -> Self {
        Self {
            handler_ms: timing.handler_ms,
            provider_ms: timing.provider_ms,
            // inference-core reports 0 when no delta arrived.
            first_token_ms: (timing.first_token_ms > 0).then_some(timing.first_token_ms),
            attempts: timing.attempts,
            response_cache_hit: timing.response_cache_hit,
            // Upstream phase names are fixed identifiers. Anything else is
            // dropped rather than repeated into a log that must stay content-free.
            upstream_ms: timing
                .upstream_ms
                .iter()
                .filter(|(name, _)| is_phase_name(name))
                .map(|(name, ms)| (name.clone(), *ms))
                .collect(),
        }
    }
}

fn is_phase_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

/// One timed stage of the turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhaseSpan {
    pub phase: Phase,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Tool-loop round, for `tool_decision`, `generation` and `dispatch`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
    /// Review pass within its draft check for `source_review` and
    /// `review_batch`; the repair's ordinal within its check for `repair`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass: Option<u32>,
    /// Batch index of a `review_batch`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<u32>,
    /// Protocol attempt of a review or repair call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    /// `ok`, `error` or `abandoned` for a provider call; `accepted`, `failed`,
    /// `timeout`, `cancelled` or `abandoned` on a draft check's last
    /// `local_validation` span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<&'static str>,
    /// Offered tool names: those a round proposed, or those a dispatch ran.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderSummary>,
    /// Remaining allowance when a draft check started: the smaller of the
    /// per-check deadline and the shared tool-phase deadline. Carried by the
    /// check's first `local_validation` span only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_ms: Option<u64>,
}

impl PhaseSpan {
    #[must_use]
    pub fn new(phase: Phase, start_ms: u64, end_ms: u64) -> Self {
        Self {
            phase,
            start_ms,
            end_ms,
            round: None,
            pass: None,
            batch: None,
            attempt: None,
            outcome: None,
            tools: Vec::new(),
            provider: None,
            budget_ms: None,
        }
    }

    #[must_use]
    pub fn with_round(mut self, round: u32) -> Self {
        self.round = Some(round);
        self
    }

    #[must_use]
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self.tools.truncate(MAX_SPAN_TOOLS);
        self
    }
}

/// The `turn_timing` SSE payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnTimingReport {
    pub request_id: String,
    pub outcome: Outcome,
    pub server_ms: u64,
    pub provider_ms: u64,
    pub orchestration_ms: u64,
    pub marks: Marks,
    pub counts: Counts,
    /// Whether the token counts on this turn's `usage` event were reported by
    /// the provider: the answer call said so. False for a synthetic artifact
    /// answer, and on a route that reports no usage, where 0 means unknown.
    pub tokens_reported: bool,
    pub phases: Vec<PhaseSpan>,
    pub phases_dropped: u32,
    /// The size of the turn's document writes; absent on a turn that wrote
    /// none.
    #[serde(skip_serializing_if = "Draft::is_empty")]
    pub draft: Draft,
}

/// Total length of the union of `[start, end)` intervals: overlapping time,
/// such as parallel review batches, is counted once.
#[must_use]
pub fn union_ms(intervals: &[(u64, u64)]) -> u64 {
    let mut sorted: Vec<(u64, u64)> = intervals
        .iter()
        .copied()
        .filter(|(start, end)| end > start)
        .collect();
    sorted.sort_unstable();
    let mut total = 0;
    let mut current: Option<(u64, u64)> = None;
    for (start, end) in sorted {
        match current.as_mut() {
            Some((_, current_end)) if start <= *current_end => {
                *current_end = (*current_end).max(end);
            }
            _ => {
                if let Some((open, close)) = current.replace((start, end)) {
                    total += close - open;
                }
            }
        }
    }
    if let Some((open, close)) = current {
        total += close - open;
    }
    total
}

/// The phase a tool round counts as, from the offered tools it proposed: a
/// round that writes a document is the author call.
pub fn round_phase<'a>(tools: impl IntoIterator<Item = &'a str>) -> Phase {
    if tools
        .into_iter()
        .any(|tool| matches!(tool, "create_artifact" | "update_artifact"))
    {
        Phase::Generation
    } else {
        Phase::ToolDecision
    }
}

fn ms(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// The turn's clock. Cheap to clone; every clone shares one ledger, and the
/// `turn timing` log line is written when the last clone drops.
#[derive(Clone)]
pub struct TurnTiming(Arc<Clock>);

struct Clock {
    request_id: String,
    origin: Instant,
    ledger: Mutex<Ledger>,
}

struct OpenCall {
    span: PhaseSpan,
}

#[derive(Default)]
struct Ledger {
    phases: Vec<PhaseSpan>,
    phases_dropped: u32,
    marks: Marks,
    counts: Counts,
    draft: Draft,
    provider_intervals: Vec<(u64, u64)>,
    open_calls: BTreeMap<u64, OpenCall>,
    next_call: u64,
    /// (check, candidate hash, iteration) → pass ordinal within that check.
    /// `iteration` (the `check_artifact` attempt the id was minted for; `0`
    /// when its id carries no marker) keeps a re-review of a byte-identical
    /// candidate — a rejected PRESERVED segment's edit `Preservation::apply`
    /// reverted — a distinct pass, never folded into an earlier one by the
    /// candidate hash alone (owner ruling 2026-09-27, truthful counts).
    passes: BTreeMap<(String, String, u32), u32>,
    /// (check, candidate hash, iteration, batch).
    batches: BTreeSet<(String, String, u32, u32)>,
    /// (check, repair kind, iteration) → repair ordinal within that check.
    repairs: BTreeMap<(String, char, u32), u32>,
    /// Per check: the last reviewed candidate, and whether a repair ran since.
    last_review: BTreeMap<String, (String, bool)>,
    tokens_reported: bool,
    outcome: Option<Outcome>,
    handed_off: Option<&'static str>,
    reported: bool,
    run_id: String,
    tools: BTreeSet<String>,
}

impl Ledger {
    fn push(&mut self, span: PhaseSpan) {
        self.tools.extend(span.tools.iter().cloned());
        if self.phases.len() < MAX_PHASES {
            self.phases.push(span);
        } else {
            self.phases_dropped = self.phases_dropped.saturating_add(1);
        }
    }

    fn close_call(
        &mut self,
        id: u64,
        end_ms: u64,
        outcome: &'static str,
        provider: Option<&ProviderTiming>,
    ) -> bool {
        let Some(OpenCall { mut span, .. }) = self.open_calls.remove(&id) else {
            return false;
        };
        span.end_ms = end_ms.max(span.start_ms);
        span.outcome = Some(outcome);
        span.provider = provider.map(ProviderSummary::from);
        self.provider_intervals.push((span.start_ms, span.end_ms));
        self.counts.provider_calls = self.counts.provider_calls.saturating_add(1);
        self.push(span);
        true
    }

    /// Close the calls still open when the turn reports: its exit cut them off.
    fn abandon_open_calls(&mut self, now_ms: u64) {
        let open: Vec<u64> = self.open_calls.keys().copied().collect();
        for id in open {
            self.close_call(id, now_ms, "abandoned", None);
        }
    }

    /// Classify a validation call by the id the validation code minted for it,
    /// and count it.
    fn classify(&mut self, request_id: &str, span: &mut PhaseSpan) {
        use crate::source_validation::CallPhase;
        let Some(phase) = crate::source_validation::call_phase(request_id) else {
            return;
        };
        match phase {
            CallPhase::SourceReview {
                check,
                candidate,
                iteration,
                batch,
                attempt,
            } => {
                let key = (check.to_owned(), candidate.to_owned(), iteration);
                let next = u32::try_from(
                    self.passes
                        .keys()
                        .filter(|(owner, _, _)| owner == check)
                        .count(),
                )
                .unwrap_or(u32::MAX);
                let pass = *self.passes.entry(key).or_insert(next);
                // A new candidate reviewed with no repair since the previous
                // pass of the same check was produced by the reviewer's own
                // proposed edits: that is the only other way the check loop
                // moves on to a different candidate.
                if let Some((previous, repaired)) = self.last_review.get(check) {
                    if previous != candidate && !repaired {
                        self.counts.reviewer_edits = self.counts.reviewer_edits.saturating_add(1);
                    }
                }
                self.last_review
                    .insert(check.to_owned(), (candidate.to_owned(), false));
                if let Some(batch) = batch {
                    self.batches
                        .insert((check.to_owned(), candidate.to_owned(), iteration, batch));
                }
                // Attempt 0 is the first try; only a protocol rejection sends
                // the same check/candidate/batch again (`review_candidate`,
                // `review_in_batches`), so any later attempt is that retry.
                if attempt > 0 {
                    self.counts.protocol_retries = self.counts.protocol_retries.saturating_add(1);
                }
                span.phase = if batch.is_some() {
                    Phase::ReviewBatch
                } else {
                    Phase::SourceReview
                };
                span.pass = Some(pass);
                span.batch = batch;
                span.attempt = Some(attempt);
            }
            CallPhase::Repair {
                check,
                iteration,
                attempt,
            } => {
                span.pass = Some(self.repair_ordinal(check, 'a', iteration));
                span.attempt = Some(attempt);
                span.phase = Phase::Repair;
            }
            CallPhase::WordRepair { check, attempt } => {
                span.pass = Some(self.repair_ordinal(check, 'w', attempt));
                span.phase = Phase::Repair;
            }
        }
    }

    fn repair_ordinal(&mut self, check: &str, kind: char, iteration: u32) -> u32 {
        if let Some((_, repaired)) = self.last_review.get_mut(check) {
            *repaired = true;
        }
        let next = u32::try_from(
            self.repairs
                .keys()
                .filter(|(owner, _, _)| owner == check)
                .count(),
        )
        .unwrap_or(u32::MAX);
        *self
            .repairs
            .entry((check.to_owned(), kind, iteration))
            .or_insert(next)
    }

    fn counts(&self) -> Counts {
        Counts {
            review_passes: u32::try_from(self.passes.len()).unwrap_or(u32::MAX),
            review_batches: u32::try_from(self.batches.len()).unwrap_or(u32::MAX),
            repairs: u32::try_from(self.repairs.len()).unwrap_or(u32::MAX),
            ..self.counts.clone()
        }
    }

    fn snapshot(&self, request_id: &str, outcome: Outcome, now_ms: u64) -> TurnTimingReport {
        let provider_ms = union_ms(&self.provider_intervals);
        TurnTimingReport {
            request_id: request_id.to_owned(),
            outcome,
            server_ms: now_ms,
            provider_ms,
            orchestration_ms: now_ms.saturating_sub(provider_ms),
            marks: self.marks.clone(),
            counts: self.counts(),
            tokens_reported: self.tokens_reported,
            phases: self.phases.clone(),
            phases_dropped: self.phases_dropped,
            draft: self.draft.clone(),
        }
    }
}

impl TurnTiming {
    /// Start the clock for the turn `request_id` names. Its origin is now.
    #[must_use]
    pub fn start(request_id: &str) -> Self {
        Self(Arc::new(Clock {
            request_id: request_id.to_owned(),
            origin: Instant::now(),
            ledger: Mutex::new(Ledger::default()),
        }))
    }

    fn ledger(&self) -> MutexGuard<'_, Ledger> {
        // Timing is advisory: a poisoned ledger still reports what it holds.
        self.0.ledger.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Milliseconds since the turn origin.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        ms(self.0.origin.elapsed())
    }

    /// Record `mark` now, unless it was already reached.
    pub fn mark(&self, mark: Mark) {
        let now = self.now_ms();
        self.ledger().marks.slot(mark).get_or_insert(now);
    }

    /// Record `phase` from `since_ms` until now, and return now.
    pub fn close(&self, phase: Phase, since_ms: u64) -> u64 {
        let now = self.now_ms();
        self.record(PhaseSpan::new(phase, since_ms, now));
        now
    }

    /// Keep `span`, or count it once the span list is full.
    pub fn record(&self, span: PhaseSpan) {
        self.ledger().push(span);
    }

    /// The durable run this turn belongs to, for the log line.
    pub fn set_run(&self, run_id: &str) {
        run_id.clone_into(&mut self.ledger().run_id);
    }

    /// The turn left this clock for another stream path (`route`) that does
    /// not report timing. The log line says so instead of guessing an outcome.
    pub fn hand_off(&self, route: &'static str) {
        self.ledger().handed_off = Some(route);
    }

    /// Record a failed outcome unless one is already known: for a request
    /// rejected before its stream task started.
    pub fn fail(&self) {
        self.ledger().outcome.get_or_insert(Outcome::Failed);
    }

    /// Whether the answer call's token counts, which the `usage` event
    /// carries, were reported by the provider.
    pub fn answer_tokens(&self, timing: Option<&ProviderTiming>) {
        self.ledger().tokens_reported = timing.is_some_and(|timing| timing.usage_reported);
    }

    /// Count `writes` document writes a tool round's model output proposed,
    /// and the `chars` characters of their raw `content` arguments. Called as
    /// the round's answer arrives, before the gateway checks, splices or drops
    /// anything.
    pub fn model_document_writes(&self, writes: u32, chars: u64) {
        let mut ledger = self.ledger();
        ledger.draft.model_writes = ledger.draft.model_writes.saturating_add(writes);
        ledger.draft.model_write_chars = ledger.draft.model_write_chars.saturating_add(chars);
    }

    /// The document a write stored, `chars` characters long after every check
    /// and splice. The last one of the turn is kept.
    pub fn stored_document(&self, chars: u64) {
        self.ledger().draft.stored_chars = chars;
    }

    /// The model's document write was a section alone, spliced into the
    /// accepted document.
    pub fn fragment_spliced(&self) {
        self.ledger().draft.fragment_spliced = true;
    }

    fn open(&self, mut span: PhaseSpan, classify: Option<&str>) -> u64 {
        let mut ledger = self.ledger();
        if let Some(request_id) = classify {
            ledger.classify(request_id, &mut span);
        }
        let id = ledger.next_call;
        ledger.next_call += 1;
        ledger.open_calls.insert(id, OpenCall { span });
        id
    }

    /// Count `calls` provider calls made inside an already recorded phase,
    /// from `since_ms` until now, without spans of their own.
    pub fn provider_interval(&self, since_ms: u64, calls: u32) {
        let now = self.now_ms();
        let mut ledger = self.ledger();
        ledger.provider_intervals.push((since_ms, now));
        ledger.counts.provider_calls = ledger.counts.provider_calls.saturating_add(calls);
    }

    /// Start timing one provider call in `phase`.
    pub fn call(&self, phase: Phase) -> ProviderCall {
        let now = self.now_ms();
        let id = self.open(PhaseSpan::new(phase, now, now), None);
        ProviderCall {
            timing: self.clone(),
            id,
            check: None,
        }
    }

    /// Start timing tool round `round`. It counts as a `tool_decision` until
    /// [`ProviderCall::set_phase`] says otherwise.
    pub fn round(&self, round: u32) -> ProviderCall {
        {
            let mut ledger = self.ledger();
            ledger.counts.model_rounds = ledger.counts.model_rounds.saturating_add(1);
        }
        let now = self.now_ms();
        let id = self.open(
            PhaseSpan::new(Phase::ToolDecision, now, now).with_round(round),
            None,
        );
        ProviderCall {
            timing: self.clone(),
            id,
            check: None,
        }
    }

    /// Start timing one draft check, with `budget_ms` of its allowance left.
    /// Calls whose id carries no validation tag count as `default_phase`.
    pub fn check(&self, default_phase: Phase, budget_ms: Option<u64>) -> CheckClock {
        CheckClock(Arc::new(CheckInner {
            timing: self.clone(),
            default_phase,
            state: Mutex::new(CheckState {
                in_flight: 0,
                gap_start: self.now_ms(),
                first: true,
                budget_ms,
                finished: false,
            }),
        }))
    }

    /// The `turn_timing` payload, the first time it is asked for: it is
    /// emitted once, just before the terminal frame. Records `outcome` and
    /// closes any provider call the exit cut off as `abandoned`.
    pub fn report(&self, outcome: Outcome) -> Option<TurnTimingReport> {
        let now = self.now_ms();
        let mut ledger = self.ledger();
        if ledger.reported {
            return None;
        }
        ledger.reported = true;
        let outcome = *ledger.outcome.get_or_insert(outcome);
        ledger.abandon_open_calls(now);
        Some(ledger.snapshot(&self.0.request_id, outcome, now))
    }

    /// The `USAGE_ENVELOPE` summary: the report without its span list, as of
    /// now. The envelope is built after durable completion, so a title or
    /// follow-up call that runs afterwards is only in the event and the log.
    #[must_use]
    pub fn summary(&self) -> serde_json::Value {
        let now = self.now_ms();
        let ledger = self.ledger();
        let provider_ms = union_ms(&ledger.provider_intervals);
        let mut summary = serde_json::json!({
            "server_ms": now,
            "provider_ms": provider_ms,
            "orchestration_ms": now.saturating_sub(provider_ms),
            "marks": ledger.marks,
            "counts": ledger.counts(),
            "tokens_reported": ledger.tokens_reported,
        });
        if !ledger.draft.is_empty() {
            summary["draft"] = serde_json::json!(ledger.draft);
        }
        summary
    }
}

impl Drop for Clock {
    fn drop(&mut self) {
        let now = ms(self.origin.elapsed());
        let ledger = self
            .ledger
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        ledger.abandon_open_calls(now);
        let outcome = ledger.outcome.unwrap_or(Outcome::Cancelled);
        let line = TimingLine {
            report: ledger.snapshot(&self.request_id, outcome, now),
            run_id: std::mem::take(&mut ledger.run_id),
            handed_off: ledger.handed_off,
            tools: ledger.tools.iter().cloned().collect::<Vec<_>>().join(","),
        };
        line.write();
    }
}

/// The `turn timing` log line.
#[derive(Debug, Clone)]
struct TimingLine {
    report: TurnTimingReport,
    run_id: String,
    handed_off: Option<&'static str>,
    tools: String,
}

#[cfg(test)]
thread_local! {
    static WRITTEN: std::cell::RefCell<Vec<TimingLine>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl TimingLine {
    fn write(self) {
        let report = &self.report;
        let counts = &report.counts;
        let marks = &report.marks;
        tracing::info!(
            request_id = %report.request_id,
            run_id = %self.run_id,
            outcome = self.handed_off.map_or(report.outcome.as_str(), |_| "handed_off"),
            route = self.handed_off,
            server_ms = report.server_ms,
            provider_ms = report.provider_ms,
            orchestration_ms = report.orchestration_ms,
            connected_ms = marks.connected_ms,
            source_ready_ms = marks.source_ready_ms,
            first_token_ms = marks.first_token_ms,
            first_draft_ms = marks.first_draft_ms,
            first_validated_ms = marks.first_validated_ms,
            durable_ms = marks.durable_ms,
            model_rounds = counts.model_rounds,
            provider_calls = counts.provider_calls,
            review_passes = counts.review_passes,
            review_batches = counts.review_batches,
            repairs = counts.repairs,
            reviewer_edits = counts.reviewer_edits,
            tokens_reported = report.tokens_reported,
            draft_model_writes = report.draft.model_writes,
            draft_model_write_chars = report.draft.model_write_chars,
            draft_stored_chars = report.draft.stored_chars,
            draft_fragment_spliced = report.draft.fragment_spliced,
            phases = report.phases.len(),
            phases_dropped = report.phases_dropped,
            tools = %self.tools,
            "turn timing"
        );
        #[cfg(test)]
        WRITTEN.with(|written| written.borrow_mut().push(self));
    }
}

/// One timed provider call. Recorded when it finishes, or as `abandoned` when
/// it is dropped first: a deadline, a Stop or a failed sibling cut it off.
#[must_use = "a provider call is recorded when it finishes or drops"]
pub struct ProviderCall {
    timing: TurnTiming,
    id: u64,
    check: Option<Arc<CheckInner>>,
}

impl ProviderCall {
    /// Change the phase this call counts as, once its result says what it was.
    pub fn set_phase(&self, phase: Phase) {
        if let Some(call) = self.timing.ledger().open_calls.get_mut(&self.id) {
            call.span.phase = phase;
        }
    }

    /// The offered tool names this call proposed.
    pub fn set_tools(&self, tools: Vec<String>) {
        if let Some(call) = self.timing.ledger().open_calls.get_mut(&self.id) {
            call.span.tools = tools;
            call.span.tools.truncate(MAX_SPAN_TOOLS);
        }
    }

    /// Record the call as ended now, with inference-core's report if any.
    pub fn finish(self, outcome: &'static str, provider: Option<&ProviderTiming>) {
        self.close(outcome, provider);
    }

    /// [`Self::finish`] from a unary result.
    pub fn finish_response(self, response: &Result<InferResponse, tonic::Status>) {
        match response {
            Ok(response) => self.finish("ok", response.provider_timing.as_ref()),
            Err(_) => self.finish("error", None),
        }
    }

    fn close(&self, outcome: &'static str, provider: Option<&ProviderTiming>) {
        let now = self.timing.now_ms();
        let closed = self
            .timing
            .ledger()
            .close_call(self.id, now, outcome, provider);
        if closed {
            if let Some(check) = &self.check {
                check.call_ended(now);
            }
        }
    }
}

impl Drop for ProviderCall {
    fn drop(&mut self) {
        self.close("abandoned", None);
    }
}

/// Timing for one draft check: classifies its provider calls and records the
/// gateway-local work between them as `local_validation`.
#[derive(Clone)]
pub struct CheckClock(Arc<CheckInner>);

struct CheckInner {
    timing: TurnTiming,
    default_phase: Phase,
    state: Mutex<CheckState>,
}

struct CheckState {
    in_flight: u32,
    gap_start: u64,
    first: bool,
    budget_ms: Option<u64>,
    finished: bool,
}

impl CheckClock {
    fn state(&self) -> MutexGuard<'_, CheckState> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Start one provider call of this check, classified by the id the
    /// validation code minted for it.
    pub fn call(&self, request_id: &str) -> ProviderCall {
        let now = self.0.timing.now_ms();
        {
            let mut state = self.state();
            if state.in_flight == 0 && !state.finished {
                self.0.close_gap(&mut state, now, None);
            }
            state.in_flight += 1;
        }
        let id = self.0.timing.open(
            PhaseSpan::new(self.0.default_phase, now, now),
            Some(request_id),
        );
        ProviderCall {
            timing: self.0.timing.clone(),
            id,
            check: Some(self.0.clone()),
        }
    }

    /// End the check with `outcome`, closing its last `local_validation` span.
    pub fn finish(&self, outcome: &'static str) {
        let now = self.0.timing.now_ms();
        let mut state = self.state();
        self.0.finish(&mut state, now, outcome);
    }
}

impl CheckInner {
    /// Close the current no-call interval. The first one is always kept (it
    /// carries the allowance), the last one too (it carries the outcome), and
    /// the ones between only when at least a millisecond long.
    fn close_gap(&self, state: &mut CheckState, now: u64, outcome: Option<&'static str>) {
        let first = std::mem::replace(&mut state.first, false);
        if first || outcome.is_some() || now > state.gap_start {
            let mut span = PhaseSpan::new(
                Phase::LocalValidation,
                state.gap_start,
                now.max(state.gap_start),
            );
            if first {
                span.budget_ms = state.budget_ms;
            }
            span.outcome = outcome;
            self.timing.record(span);
        }
        state.gap_start = now;
    }

    fn call_ended(&self, now: u64) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.in_flight = state.in_flight.saturating_sub(1);
        if state.in_flight == 0 {
            state.gap_start = now;
        }
    }

    fn finish(&self, state: &mut CheckState, now: u64, outcome: &'static str) {
        if state.finished {
            return;
        }
        state.finished = true;
        if state.in_flight > 0 {
            state.gap_start = now;
        }
        self.close_gap(state, now, Some(outcome));
    }
}

impl Drop for CheckInner {
    fn drop(&mut self) {
        let now = self.timing.now_ms();
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        if !state.finished {
            state.finished = true;
            let first = std::mem::replace(&mut state.first, false);
            let mut span = PhaseSpan::new(
                Phase::LocalValidation,
                state.gap_start,
                now.max(state.gap_start),
            );
            if first {
                span.budget_ms = state.budget_ms;
            }
            span.outcome = Some("abandoned");
            self.timing.record(span);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written() -> Vec<TimingLine> {
        WRITTEN.with(|written| std::mem::take(&mut *written.borrow_mut()))
    }

    fn keys(value: &serde_json::Value, path: &str, out: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    // Upstream phase names are data keys, checked separately.
                    if path.ends_with("upstream_ms") {
                        continue;
                    }
                    let path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    out.insert(path.clone());
                    keys(value, &path, out);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    keys(item, &format!("{path}[]"), out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn overlapping_provider_intervals_count_once() {
        assert_eq!(union_ms(&[(0, 10), (2, 8), (5, 15)]), 15);
        assert_eq!(
            union_ms(&[(10, 12), (0, 5)]),
            7,
            "disjoint intervals add up"
        );
        assert_eq!(union_ms(&[(0, 5), (5, 9)]), 9, "touching intervals merge");
        assert_eq!(union_ms(&[]), 0);
        assert_eq!(
            union_ms(&[(4, 4), (9, 3)]),
            0,
            "empty and inverted intervals are ignored"
        );
    }

    #[test]
    fn orchestration_time_is_never_negative() {
        let ledger = Ledger {
            // A clock read is never earlier than a call it closed, but the
            // arithmetic must not rely on that.
            provider_intervals: vec![(0, 50)],
            ..Ledger::default()
        };
        let report = ledger.snapshot("req", Outcome::Completed, 20);
        assert_eq!(report.provider_ms, 50);
        assert_eq!(report.orchestration_ms, 0);
    }

    #[test]
    fn a_turn_dropped_without_an_outcome_logs_one_cancelled_line() {
        let _ = written();
        let timing = TurnTiming::start("req-dropped");
        timing.set_run("run-1");
        let clone = timing.clone();
        drop(timing);
        assert!(written().is_empty(), "the line waits for the last handle");
        drop(clone);
        let lines = written();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].report.outcome, Outcome::Cancelled);
        assert_eq!(lines[0].report.request_id, "req-dropped");
        assert_eq!(lines[0].run_id, "run-1");

        let timing = TurnTiming::start("req-done");
        assert!(timing.report(Outcome::Timeout).is_some());
        assert!(
            timing.report(Outcome::Completed).is_none(),
            "the event is emitted once"
        );
        drop(timing);
        let lines = written();
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].report.outcome,
            Outcome::Timeout,
            "the reported outcome is logged"
        );

        let timing = TurnTiming::start("req-rejected");
        timing.fail();
        drop(timing);
        assert_eq!(written()[0].report.outcome, Outcome::Failed);

        let timing = TurnTiming::start("req-zdr");
        timing.hand_off("zdr_direct");
        drop(timing);
        assert_eq!(written()[0].handed_off, Some("zdr_direct"));
    }

    #[test]
    fn document_rounds_are_generation() {
        assert_eq!(round_phase(["create_artifact"]), Phase::Generation);
        assert_eq!(
            round_phase(["web_search", "update_artifact"]),
            Phase::Generation
        );
        assert_eq!(
            round_phase(["read_artifact", "count_words"]),
            Phase::ToolDecision
        );
        assert_eq!(round_phase([]), Phase::ToolDecision);
    }

    #[tokio::test(start_paused = true)]
    async fn tool_rounds_and_answer_calls_are_timed_on_the_turn_origin() {
        let timing = TurnTiming::start("req");
        tokio::time::advance(std::time::Duration::from_millis(100)).await;
        let round = timing.round(0);
        tokio::time::advance(std::time::Duration::from_secs(20)).await;
        round.set_phase(round_phase(["create_artifact"]));
        round.set_tools(vec!["create_artifact".into()]);
        round.finish(
            "ok",
            Some(&ProviderTiming {
                handler_ms: 19_990,
                provider_ms: 19_950,
                attempts: 1,
                ..Default::default()
            }),
        );
        timing.mark(Mark::FirstDraft);
        tokio::time::advance(std::time::Duration::from_millis(30)).await;
        let abandoned = timing.call(Phase::AnswerStream);
        tokio::time::advance(std::time::Duration::from_millis(5)).await;
        let report = timing.report(Outcome::Cancelled).expect("first report");
        drop(abandoned);
        assert_eq!(report.server_ms, 20_135);
        assert_eq!(report.provider_ms, 20_005);
        assert_eq!(report.orchestration_ms, 130);
        assert_eq!(report.marks.first_draft_ms, Some(20_100));
        assert_eq!(report.counts.model_rounds, 1);
        assert_eq!(report.counts.provider_calls, 2);
        let generation = &report.phases[0];
        assert_eq!(
            (
                generation.phase,
                generation.round,
                generation.start_ms,
                generation.end_ms
            ),
            (Phase::Generation, Some(0), 100, 20_100)
        );
        assert_eq!(
            generation
                .provider
                .as_ref()
                .map(|provider| provider.handler_ms),
            Some(19_990)
        );
        assert_eq!(
            generation
                .provider
                .as_ref()
                .and_then(|provider| provider.first_token_ms),
            None,
            "0 means no delta"
        );
        assert_eq!(
            report.phases[1].outcome,
            Some("abandoned"),
            "the report closes calls its exit cut off"
        );
    }

    #[test]
    fn the_span_list_is_bounded_and_counts_what_it_dropped() {
        let timing = TurnTiming::start("req");
        for index in 0..70 {
            timing.record(PhaseSpan::new(Phase::Dispatch, index, index + 1));
        }
        let report = timing.report(Outcome::Completed).unwrap();
        assert_eq!(report.phases.len(), MAX_PHASES);
        assert_eq!(report.phases_dropped, 6);
        assert_eq!(
            report.phases[63].start_ms, 63,
            "the earliest spans are the ones kept"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_draft_check_classifies_its_calls_and_keeps_local_work_between_them() {
        let timing = TurnTiming::start("req");
        let check = timing.check(Phase::SourceReview, Some(90_000));
        let step = |millis| tokio::time::advance(std::time::Duration::from_millis(millis));
        step(4).await;
        // A split first pass: two parallel batches.
        let hash_a = "0123456789abcdef";
        let first = check.call(&format!("req-call-source-review-{hash_a}-batch-0-0"));
        let second = check.call(&format!("req-call-source-review-{hash_a}-batch-1-0"));
        step(10_000).await;
        first.finish("ok", None);
        step(6_000).await;
        second.finish("ok", None);
        step(3).await;
        // Reviewer edits produce a second candidate, reviewed in one call.
        let hash_b = "fedcba9876543210";
        check
            .call(&format!("req-call-source-review-{hash_b}-0"))
            .finish("ok", None);
        // A repair, then the accepted third pass.
        step(2).await;
        let repair = check.call("req-call-artifact-repair-1-protocol-0");
        step(7_000).await;
        repair.finish("ok", None);
        check
            .call("req-call-source-review-00000000ffffffff-0")
            .finish("ok", None);
        check.finish("accepted");
        drop(check);

        let report = timing.report(Outcome::Completed).unwrap();
        assert_eq!(report.provider_ms, 23_000, "parallel batches count once");
        let counts = &report.counts;
        assert_eq!(
            (
                counts.review_passes,
                counts.review_batches,
                counts.repairs,
                counts.reviewer_edits,
                counts.provider_calls
            ),
            (3, 2, 1, 1, 5)
        );
        let local: Vec<_> = report
            .phases
            .iter()
            .filter(|span| span.phase == Phase::LocalValidation)
            .collect();
        assert_eq!(
            local[0].budget_ms,
            Some(90_000),
            "the first span carries the allowance"
        );
        assert_eq!((local[0].start_ms, local[0].end_ms), (0, 4));
        assert_eq!(local.last().unwrap().outcome, Some("accepted"));
        assert!(local.iter().all(|span| span.end_ms > span.start_ms
            || span.budget_ms.is_some()
            || span.outcome.is_some()));
        let batches: Vec<_> = report
            .phases
            .iter()
            .filter(|span| span.phase == Phase::ReviewBatch)
            .map(|span| (span.pass, span.batch, span.attempt))
            .collect();
        assert_eq!(
            batches,
            [(Some(0), Some(0), Some(0)), (Some(0), Some(1), Some(0))]
        );
        let repair = report
            .phases
            .iter()
            .find(|span| span.phase == Phase::Repair)
            .unwrap();
        assert_eq!((repair.pass, repair.attempt), (Some(0), Some(0)));
        let last = report
            .phases
            .iter()
            .rev()
            .find(|span| span.phase == Phase::SourceReview)
            .unwrap();
        assert_eq!(last.pass, Some(2));
    }

    #[test]
    fn protocol_retries_and_batches_of_one_candidate_are_one_pass() {
        let timing = TurnTiming::start("req");
        let check = timing.check(Phase::SourceReview, None);
        for id in [
            "r-source-review-0123456789abcdef-0",
            "r-source-review-0123456789abcdef-1",
        ] {
            check.call(id).finish("ok", None);
        }
        // Two different checks (two artifacts in one round) never count as
        // reviewer edits of each other.
        let other = timing.check(Phase::SourceReview, None);
        other
            .call("s-source-review-fedcba9876543210-0")
            .finish("ok", None);
        let counts = timing.report(Outcome::Completed).unwrap().counts;
        assert_eq!((counts.review_passes, counts.reviewer_edits), (2, 0));
    }

    /// Neither `review_passes` (distinct candidates) nor `review_batches`
    /// (distinct batches) counts how many attempts reaching an accepted one
    /// took: a batch retried after "did not cover every segment" reuses its
    /// own batch number, so it is invisible to both. `protocol_retries`
    /// counts exactly those extra attempts, whole-candidate or per-batch.
    #[test]
    fn protocol_retries_count_every_attempt_after_the_first() {
        let timing = TurnTiming::start("req");
        let check = timing.check(Phase::SourceReview, None);
        // A single-shot review retried once after a protocol rejection.
        for id in [
            "r-source-review-0123456789abcdef-0",
            "r-source-review-0123456789abcdef-1",
        ] {
            check.call(id).finish("ok", None);
        }
        // A split review whose batch 0 alone needed one retry; batch 1 needed none.
        for id in [
            "r-source-review-fedcba9876543210-batch-0-0",
            "r-source-review-fedcba9876543210-batch-0-1",
            "r-source-review-fedcba9876543210-batch-1-0",
        ] {
            check.call(id).finish("ok", None);
        }
        let counts = timing.report(Outcome::Completed).unwrap().counts;
        assert_eq!(
            counts.protocol_retries, 2,
            "one single-shot retry and one batch retry"
        );
        assert_eq!(counts.review_passes, 2, "two distinct candidates reviewed");
        assert_eq!(
            counts.review_batches, 2,
            "two distinct batches of the split candidate, retries included"
        );
    }

    /// Owner ruling 2026-09-27, truthful counts: a later `check_artifact`
    /// attempt reviewing a BYTE-IDENTICAL candidate (a rejected PRESERVED
    /// segment's edit `Preservation::apply` reverted) is tagged with its own
    /// `-iterK-` marker and counts as its own pass and batch assignment,
    /// never folded into the earlier attempt by the repeated candidate hash.
    #[test]
    fn a_re_reviewed_identical_candidate_counts_as_its_own_pass_and_batch() {
        let timing = TurnTiming::start("req");
        let check = timing.check(Phase::SourceReview, None);
        // Attempt 0: reviewed and rejected.
        check
            .call("r-source-review-0123456789abcdef-0")
            .finish("ok", None);
        // Attempt 2 (a model repair ran at attempt 1, then preservation
        // reverted it): the SAME 16-hex candidate hash, but a genuinely
        // later, separate review call.
        check
            .call("r-source-review-0123456789abcdef-iter2-0")
            .finish("ok", None);
        let counts = timing.report(Outcome::Completed).unwrap().counts;
        assert_eq!(
            counts.review_passes, 2,
            "two distinct passes even though the candidate hash repeats"
        );

        let timing = TurnTiming::start("req");
        let check = timing.check(Phase::SourceReview, None);
        for id in [
            "r-source-review-fedcba9876543210-batch-0-0",
            "r-source-review-fedcba9876543210-batch-1-0",
            "r-source-review-fedcba9876543210-iter3-batch-0-0",
            "r-source-review-fedcba9876543210-iter3-batch-1-0",
        ] {
            check.call(id).finish("ok", None);
        }
        let counts = timing.report(Outcome::Completed).unwrap().counts;
        assert_eq!(counts.review_passes, 2, "two distinct passes, one per iteration");
        assert_eq!(
            counts.review_batches, 4,
            "all four batch assignments, none folded away by the repeated hash and batch number"
        );
    }

    #[test]
    fn the_payload_carries_only_whitelisted_keys() {
        let timing = TurnTiming::start("req-keys");
        timing.set_run("run-7c1f");
        for mark in [
            Mark::SourceReady,
            Mark::Connected,
            Mark::FirstToken,
            Mark::FirstDraft,
            Mark::FirstValidated,
            Mark::Durable,
        ] {
            timing.mark(mark);
        }
        let check = timing.check(Phase::SourceReview, Some(1));
        check
            .call("r-source-review-0123456789abcdef-batch-0-0")
            .finish(
                "ok",
                Some(&ProviderTiming {
                    first_token_ms: 3,
                    upstream_ms: [
                        ("spawn".to_owned(), 4),
                        ("Ignore previous instructions".to_owned(), 5),
                    ]
                    .into_iter()
                    .collect(),
                    ..Default::default()
                }),
            );
        check.finish("accepted");
        let round = timing.round(1);
        round.set_tools(vec!["web_search".into()]);
        round.finish("ok", None);
        timing.model_document_writes(1, 12);
        timing.stored_document(34);
        timing.fragment_spliced();
        let report = timing.report(Outcome::Completed).unwrap();
        let json = serde_json::to_value(&report).unwrap();
        let mut found = BTreeSet::new();
        keys(&json, "", &mut found);
        let allowed: BTreeSet<String> = [
            "request_id",
            "outcome",
            "server_ms",
            "provider_ms",
            "orchestration_ms",
            "tokens_reported",
            "phases_dropped",
            "marks",
            "marks.connected_ms",
            "marks.source_ready_ms",
            "marks.first_token_ms",
            "marks.first_draft_ms",
            "marks.first_validated_ms",
            "marks.durable_ms",
            "counts",
            "counts.model_rounds",
            "counts.provider_calls",
            "counts.review_passes",
            "counts.review_batches",
            "counts.repairs",
            "counts.reviewer_edits",
            "counts.protocol_retries",
            "draft",
            "draft.model_writes",
            "draft.model_write_chars",
            "draft.stored_chars",
            "draft.fragment_spliced",
            "phases",
            "phases[].phase",
            "phases[].start_ms",
            "phases[].end_ms",
            "phases[].round",
            "phases[].pass",
            "phases[].batch",
            "phases[].attempt",
            "phases[].outcome",
            "phases[].tools",
            "phases[].budget_ms",
            "phases[].provider",
            "phases[].provider.handler_ms",
            "phases[].provider.provider_ms",
            "phases[].provider.first_token_ms",
            "phases[].provider.attempts",
            "phases[].provider.response_cache_hit",
            "phases[].provider.upstream_ms",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let unexpected: Vec<_> = found.difference(&allowed).collect();
        assert!(unexpected.is_empty(), "unexpected keys: {unexpected:?}");
        let upstream = &json["phases"][1]["provider"]["upstream_ms"];
        assert_eq!(
            upstream,
            &serde_json::json!({"spawn": 4}),
            "only phase names pass through"
        );
        assert_eq!(
            json["phases"][3]["tools"],
            serde_json::json!(["web_search"])
        );
        assert!(
            !json.to_string().contains("run-7c1f"),
            "the run id is for the log line only"
        );
    }

    /// The draft-size record is character counts and one flag, taken as the
    /// model's writes arrive (before any splice), kept out of the record of a
    /// turn that wrote no document, and carried by the report, the envelope
    /// summary and the log line alike.
    #[test]
    fn draft_size_is_counts_and_a_flag_and_absent_without_a_document() {
        let _ = written();
        let timing = TurnTiming::start("req-no-document");
        assert!(timing.summary().get("draft").is_none(), "an empty draft is left out of the summary");
        let report = timing.report(Outcome::Completed).unwrap();
        assert!(report.draft.is_empty());
        assert!(
            serde_json::to_value(&report).unwrap().get("draft").is_none(),
            "and out of the report, whose existing fields are all it carries"
        );
        drop(timing);
        assert_eq!(written()[0].report.draft, Draft::default());

        let timing = TurnTiming::start("req-document");
        // Two author rounds: a whole document, then a shorter correction.
        timing.model_document_writes(1, 3_800);
        timing.model_document_writes(1, 700);
        timing.model_document_writes(0, 0);
        timing.stored_document(3_900);
        timing.stored_document(3_950);
        timing.fragment_spliced();
        let expected = serde_json::json!({
            "model_writes": 2,
            "model_write_chars": 4_500,
            "stored_chars": 3_950,
            "fragment_spliced": true,
        });
        assert_eq!(timing.summary()["draft"], expected);
        let report = timing.report(Outcome::Completed).unwrap();
        assert_eq!(serde_json::to_value(&report).unwrap()["draft"], expected);
        drop(timing);
        let lines = written();
        assert_eq!(lines[0].report.draft.model_write_chars, 4_500);
        assert!(lines[0].report.draft.fragment_spliced);

        let timing = TurnTiming::start("req-whole");
        timing.model_document_writes(1, 10);
        assert!(!timing.report(Outcome::Completed).unwrap().draft.fragment_spliced, "a flag nobody raised stays down");
    }

    #[test]
    fn the_envelope_summary_has_no_span_list() {
        let timing = TurnTiming::start("req");
        timing.call(Phase::Title).finish("ok", None);
        let summary = timing.summary();
        assert!(summary.get("phases").is_none());
        assert_eq!(summary["counts"]["provider_calls"], 1);
        assert!(summary["server_ms"].as_u64().is_some());
        assert!(
            timing.report(Outcome::Completed).is_some(),
            "a summary does not use up the event"
        );
    }
}
