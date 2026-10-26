//! The writer of the durable loop's fold (build plan WS03 S6).
//!
//! The Do loop (`agent::run_rounds`) keeps its history, counters and open tool round
//! in memory, so a pause for approval or a crash discarded all of it. This module
//! turns the loop's own locals into a [`LoopFoldV1`] at three moments and saves it
//! through session-core's `SaveLoopCheckpoint`:
//!
//! * **round**: the end of a round that ran tool calls. Best effort: a failed save
//!   is skipped, the loop goes on.
//! * **`pre_effect`**: before a round dispatches any effectful call. Fail closed: the
//!   call does not start unless the fold is durable (or deliberately replaced by a
//!   tombstone, or the store is explicitly off), because a resume reconciles the
//!   effect from this fold and the effect ledger.
//! * **pause**: inside the pause for approval, after the descriptor exists and
//!   **before** `CreateApproval`, so a fast approve-and-continue can always claim
//!   it. Its `pause_ref` is the approval's idempotency key `{run_id}:{step_id}`,
//!   which is known before session-core mints the approval id. A pause fold whose
//!   content is refused is replaced by a tombstone saved as the pause fold (same
//!   `pause_ref`), so the row behind the approval is never the earlier, resumable
//!   `pre_effect` fold; the approval is raised either way.
//!
//! No `LoopState` refactor: [`Snapshot`] and [`build_fold`] read the existing
//! `run_rounds` locals. With the switch off nothing here makes an RPC or changes
//! the conversation: the writer is constructed (a pure value), stays
//! [`OffReason::SwitchOff`], and every method returns at its first line.
//!
//! The writer needs the effect ledger (`EXECUTION_EFFECT_LEDGER_ENABLED`): a fold
//! without ledger rows cannot be reconciled on resume. It also needs
//! `SESSION_CORE_LOOP_CHECKPOINT_ENABLED` on the other side; if that is off the
//! first save is refused `loop_checkpoint_disabled` and the writer goes off, which
//! behaves exactly like the switch being off.
//!
//! Content rules: the fold is conversation content, so it is never written for a
//! zero-retention run, a waived run, a delegated loop, a Space run or a stage-1
//! continuation segment; it is scrubbed per field ([`crate::scrub::scrub_fold`])
//! and `LoopFoldV1::to_bytes` (the credential gate) is the last check. Content
//! refusals (too large, credential-shaped) degrade to a tombstone; store failures
//! fail closed where an effect depends on them. Logs are content-free: ids, kind,
//! counts and closed tokens, never fold bytes or a status message.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use mp_contracts::loop_fold::{
    self as lf, Authority, FoldCall, FoldCompleted, FoldMessage, LoopFoldV1, ParkReason, PauseKind,
    PendingPause, RoundInFlight, EXT_EFFECT_LEDGER, FOLD_VERSION,
};
use mp_contracts::model_plane::v1::{self as pb, session_core_client::SessionCoreClient};
use tonic::transport::Channel;
use tracing::{info, warn};

const CHECKPOINT_ENV: &str = "EXECUTION_LOOP_CHECKPOINT_ENABLED";

#[cfg(test)]
thread_local! {
    /// A test's override of the switch. `#[tokio::test]` runs its future on the
    /// test's own thread, so a thread-local cannot leak into a parallel test the way
    /// a process-wide environment variable would.
    static TEST_SWITCH: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Holds the checkpoint switch at a fixed value for the current test thread, then
/// restores it.
#[cfg(test)]
pub(crate) struct CheckpointSwitch;

#[cfg(test)]
impl CheckpointSwitch {
    pub(crate) fn set(on: bool) -> Self {
        TEST_SWITCH.with(|cell| cell.set(Some(on)));
        Self
    }
}

#[cfg(test)]
impl Drop for CheckpointSwitch {
    fn drop(&mut self) {
        TEST_SWITCH.with(|cell| cell.set(None));
    }
}

/// Whether `EXECUTION_LOOP_CHECKPOINT_ENABLED` is on in this process. Off by
/// default; parsed by the one strict parser (`true` and nothing else).
pub(crate) fn checkpoint_enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = TEST_SWITCH.with(std::cell::Cell::get) {
        return forced;
    }
    lf::switch_from(std::env::var(CHECKPOINT_ENV).ok().as_deref())
}

/// The most a saved fold may weigh. session-core caps the plaintext at 4 MiB
/// (`MAX_FOLD_BYTES`) and tonic's default message limit is also 4 MiB (no service
/// raises it), so the request overhead needs the margin.
pub(crate) const SAVE_BUDGET_BYTES: usize = 3 * 1024 * 1024;

/// How long a round save may take. It is best effort, so it is short.
pub(crate) const ROUND_SAVE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a pre-effect or pause save may take. An effect or an approval waits on
/// it, so it is allowed to be slow rather than fail.
pub(crate) const GATED_SAVE_TIMEOUT: Duration = Duration::from_secs(10);

/// The step-text the model reads when an effectful call is held back because the
/// run's state could not be saved ahead of it. Names only the tool: never a status
/// message, an id or a detail of the failure.
pub(crate) fn held_back_text(tool_name: &str) -> String {
    format!(
        "tool '{tool_name}' was not run: the run's state could not be saved ahead of it, so it \
         was held back. Try again."
    )
}

/// The same, for a writer that has failed: nothing will be saved for the rest of this
/// run, so the call is not held for a moment and it must not invite a retry. Names only
/// the tool.
pub(crate) fn held_for_good_text(tool_name: &str) -> String {
    format!(
        "tool '{tool_name}' was not run: the step could not be started safely in this run, so \
         it was held back and will not be run here."
    )
}

/// Everything that decides whether this run's writer may write at all.
// Each flag is an independent fact about the run, read once into a gate: a state machine
// would only rename them.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct WriterGate {
    /// `EXECUTION_LOOP_CHECKPOINT_ENABLED`.
    pub switch: bool,
    /// The effect ledger is on for this run: a fold needs ledger rows to reconcile.
    pub ledger: bool,
    /// The run is zero-retention.
    pub zdr: bool,
    /// The run is a waived run (zero-retention org, one connection with ZDR off).
    pub waived: bool,
    /// The run belongs to a Space.
    pub space: bool,
    /// Nesting depth: only the root loop (0) owns the run's fold.
    pub depth: u32,
    /// This request is a stage-1 continuation segment (it holds a release).
    pub resumed: bool,
}

/// Why a writer does not write. Closed tokens, for logs and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OffReason {
    SwitchOff,
    LedgerOff,
    Zdr,
    Waived,
    Space,
    Delegated,
    ResumedSegment,
    NoBearer,
    /// session-core answered `loop_checkpoint_disabled`.
    StoreDisabled,
    /// session-core refused the run as not persistent (a ZDR-catalogue refusal).
    RunNotPersistent,
}

impl OffReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::SwitchOff => "switch_off",
            Self::LedgerOff => "ledger_off",
            Self::Zdr => "zdr",
            Self::Waived => "waived",
            Self::Space => "space",
            Self::Delegated => "delegated",
            Self::ResumedSegment => "resumed_segment",
            Self::NoBearer => "no_bearer",
            Self::StoreDisabled => "store_disabled",
            Self::RunNotPersistent => "run_not_persistent",
        }
    }
}

impl WriterGate {
    /// `None` when the run may write a fold; otherwise why not. Pure: none of these
    /// makes an RPC.
    pub(crate) const fn decide(&self) -> Option<OffReason> {
        if !self.switch {
            Some(OffReason::SwitchOff)
        } else if !self.ledger {
            Some(OffReason::LedgerOff)
        } else if self.zdr {
            Some(OffReason::Zdr)
        } else if self.waived {
            Some(OffReason::Waived)
        } else if self.space {
            Some(OffReason::Space)
        } else if self.depth > 0 {
            Some(OffReason::Delegated)
        } else if self.resumed {
            Some(OffReason::ResumedSegment)
        } else {
            None
        }
    }
}

/// Why a fold was written; the wire `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveKind {
    Round,
    PreEffect,
    Pause,
    /// A park at a round boundary (WS03 S8-A): written only by [`FoldWriter::save_park`].
    Park,
}

impl SaveKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Round => "round",
            Self::PreEffect => "pre_effect",
            Self::Pause => "pause",
            Self::Park => "park",
        }
    }

    const fn timeout(self) -> Duration {
        match self {
            Self::Round => ROUND_SAVE_TIMEOUT,
            Self::PreEffect | Self::Pause | Self::Park => GATED_SAVE_TIMEOUT,
        }
    }
}

/// How a save failed. The rule: **content refusals degrade, store failures fail
/// closed** where an effect depends on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveError {
    /// The stored seq is not the one this writer expected: a second writer exists
    /// (or this writer lost track). The writer stops.
    Stale,
    /// The fold is closed or the run has ended: nothing more may be written.
    Closed,
    /// session-core's fold store is switched off.
    StoreOff,
    /// session-core refuses this run as not persistent (ZDR, waived).
    NotPersistent,
    /// The content cannot be saved: too large, credential-shaped, or refused by
    /// session-core as invalid. `reason` is a closed word.
    ContentRefused { reason: &'static str },
    /// The store could not be reached or failed. `ambiguous` is set when the save
    /// may nevertheless have committed (a timeout, a transport failure).
    Unavailable { ambiguous: bool },
    /// session-core refused the save for a reason that will not change within this run
    /// (the run is not found, the caller is not allowed, the RPC does not exist on that
    /// session-core, it has no key). Asking again gets the same answer, so the writer
    /// fails at once and holds effects for the rest of the run.
    Refused(RefusedBy),
}

/// Why session-core refused for good ([`SaveError::Refused`]). Closed tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefusedBy {
    /// `NotFound`: the run is not known to the store.
    NotFound,
    /// `PermissionDenied` or `Unauthenticated`: the credential may not save this run's fold.
    Denied,
    /// `Unimplemented`: a session-core that predates `SaveLoopCheckpoint`.
    Unimplemented,
    /// `FailedPrecondition` "loop checkpoint key unavailable": no key to seal the fold with.
    KeyUnavailable,
}

impl RefusedBy {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Denied => "denied",
            Self::Unimplemented => "unimplemented",
            Self::KeyUnavailable => "key_unavailable",
        }
    }
}

impl SaveError {
    /// The closed token logged for this failure.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Stale => "stale",
            Self::Closed => "closed",
            Self::StoreOff => "store_off",
            Self::NotPersistent => "not_persistent",
            Self::ContentRefused { .. } => "content_refused",
            Self::Unavailable { .. } => "unavailable",
            Self::Refused(_) => "refused",
        }
    }

    /// The closed word that says why, for the failures that have one; empty otherwise.
    pub(crate) const fn cause(self) -> &'static str {
        match self {
            Self::ContentRefused { reason } => reason,
            Self::Refused(by) => by.as_str(),
            _ => "",
        }
    }
}

/// How a park save failed (WS03 S8-A). [`SaveError`] stays what it was: a park adds one
/// outcome of its own, so it is a second type instead of a new variant every other match
/// on [`SaveError`] would have to learn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParkSaveError {
    /// The save failed as any other save does.
    Save(SaveError),
    /// The save was stored under `seq` but the store did not say it accepted a park (a
    /// session-core that predates the park, or one with park recovery off, stores the fold
    /// as an ordinary row and echoes no `park_accepted`). The park was NOT taken: the run
    /// takes its committed outcome, and the caller closes the row it left behind.
    NotAccepted { seq: u64 },
}

/// Classify a session-core status. Only the closed `loop_checkpoint_*` tokens and
/// the ZDR catalogue trailers are read; the message is never copied anywhere.
pub(crate) fn classify_status(status: &tonic::Status) -> SaveError {
    use tonic::Code;
    // A ZDR-catalogue refusal (`DURABLE_MUTATION_FORBIDDEN`, a feature refusal)
    // carries its kind on a trailer whatever the code.
    if mp_contracts::zdr_refusal::Refusal::from_status(status).is_some() {
        return SaveError::NotPersistent;
    }
    match (status.code(), status.message()) {
        (Code::Aborted, "loop_checkpoint_stale") => SaveError::Stale,
        (Code::FailedPrecondition, "loop_checkpoint_closed" | "loop_checkpoint_run_ended") => {
            SaveError::Closed
        }
        (Code::FailedPrecondition, "loop_checkpoint_disabled") => SaveError::StoreOff,
        // A deterministic refusal of this run or caller: the same answer comes back next
        // time, so none of these is retried.
        (Code::NotFound, _) => SaveError::Refused(RefusedBy::NotFound),
        (Code::PermissionDenied | Code::Unauthenticated, _) => {
            SaveError::Refused(RefusedBy::Denied)
        }
        (Code::Unimplemented, _) => SaveError::Refused(RefusedBy::Unimplemented),
        (Code::FailedPrecondition, "loop checkpoint key unavailable") => {
            SaveError::Refused(RefusedBy::KeyUnavailable)
        }
        // The content itself is refused as invalid or out of range. (Throttling,
        // `ResourceExhausted`, is not about the content: it falls to the transient arm.)
        (Code::InvalidArgument | Code::OutOfRange, _) => {
            SaveError::ContentRefused { reason: "rejected" }
        }
        (
            Code::Unavailable
            | Code::DeadlineExceeded
            | Code::Cancelled
            | Code::Unknown
            | Code::Internal
            | Code::DataLoss,
            _,
        ) => SaveError::Unavailable { ambiguous: true },
        // The rest did not commit (throttling included): transient, certain.
        _ => SaveError::Unavailable { ambiguous: false },
    }
}

/// The `expected_seq` of the next save. The first save of a run expects 0; every
/// save that succeeds moves it to the seq session-core returned.
///
/// A save that failed ambiguously (a timeout) may have committed, which would make
/// the next save come back stale through no fault of a second writer. So after an
/// ambiguous failure, **one** stale answer is retried with `expected + 1`; a second
/// stale answer means another writer, and the writer stops.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SeqTracker {
    expected: u64,
    uncertain: bool,
}

impl SeqTracker {
    /// A tracker that expects `expected` next: the seq a resumed loop was handed by its
    /// claim, instead of 0 for a run's first save.
    pub(crate) const fn seeded(expected: u64) -> Self {
        Self {
            expected,
            uncertain: false,
        }
    }

    pub(crate) const fn expected(&self) -> u64 {
        self.expected
    }

    /// A save succeeded and session-core stored the fold under `seq`.
    pub(crate) fn on_saved(&mut self, seq: u64) {
        self.expected = seq;
        self.uncertain = false;
    }

    /// A save failed. Only an ambiguous failure changes anything: a certain one
    /// committed nothing, so `expected` stands.
    pub(crate) fn on_failed(&mut self, error: &SaveError) {
        if matches!(error, SaveError::Unavailable { ambiguous: true }) {
            self.uncertain = true;
        }
    }

    /// After a stale answer: the seq to retry with, once, if an earlier save may
    /// have committed; `None` when it may not (or the probe was already spent).
    pub(crate) fn probe(&mut self) -> Option<u64> {
        if self.uncertain {
            self.uncertain = false;
            Some(self.expected.saturating_add(1))
        } else {
            None
        }
    }
}

/// The loop's locals at a save point, named. **KEEP IN SYNC with the `run_rounds`
/// locals**: a loop-local added later is not saved unless it is added here, to
/// [`build_fold`] and to the test that gives every field a distinct value.
pub(crate) struct Snapshot<'a> {
    pub run_id: &'a str,
    pub segment_no: u32,
    /// Rounds started, including the open round at a pre-effect or pause save.
    pub rounds_executed: u32,
    pub delegated_rounds: u32,
    pub step_seq: u32,
    pub children_started: u32,
    pub grounded: bool,
    pub compaction_triggered: bool,
    pub attempted_retrievals: &'a BTreeSet<String>,
    pub messages: &'a [pb::ChatMessage],
    /// `resume::offered_authority(req, permission_wire, allowlist)`.
    pub authority: Authority,
    /// `loop_fold::prompt_digest` over the system prompt and the offered tools.
    pub prompt_digest: &'a str,
    /// The writer requires the effect ledger, so this is `true` for any fold that
    /// is written; it is recorded so a resume knows it may reconcile effects.
    pub effect_ledger: bool,
}

/// The shared `prompt_digest` of a run: the code-owned system prompt and the tools
/// the run was offered, as a resume will recompute them.
pub(crate) fn prompt_digest_of(system_prompt: &str, tools: &[pb::ToolDefinition]) -> String {
    let tools: Vec<(&str, &str, &str)> = tools
        .iter()
        .map(|tool| {
            (
                tool.name.as_str(),
                tool.description.as_str(),
                tool.parameters_json.as_str(),
            )
        })
        .collect();
    lf::prompt_digest(system_prompt, &tools)
}

fn fold_messages(messages: &[pb::ChatMessage]) -> Vec<FoldMessage> {
    messages
        .iter()
        .map(|message| FoldMessage {
            role: message.role.clone(),
            content: message.content.clone(),
            name: message.name.clone(),
            compaction_summary: message.compaction_summary.clone(),
        })
        .collect()
}

/// Build the fold from the loop's locals. Unscrubbed: [`FoldWriter`] scrubs and
/// gates it when it saves.
pub(crate) fn build_fold(
    snapshot: &Snapshot<'_>,
    round: Option<RoundInFlight>,
    pending: Option<PendingPause>,
) -> LoopFoldV1 {
    let mut extensions = serde_json::Map::new();
    if snapshot.effect_ledger {
        extensions.insert(EXT_EFFECT_LEDGER.to_owned(), serde_json::Value::Bool(true));
    }
    LoopFoldV1 {
        v: FOLD_VERSION,
        run_id: snapshot.run_id.to_owned(),
        segment_no: snapshot.segment_no,
        rounds_executed: snapshot.rounds_executed,
        delegated_rounds: snapshot.delegated_rounds,
        step_seq: snapshot.step_seq,
        children_started: snapshot.children_started,
        grounded: snapshot.grounded,
        compaction_triggered: snapshot.compaction_triggered,
        attempted_retrievals: snapshot.attempted_retrievals.iter().cloned().collect(),
        messages: fold_messages(snapshot.messages),
        round_in_flight: round,
        pending,
        authority: snapshot.authority.clone(),
        prompt_digest: snapshot.prompt_digest.to_owned(),
        extensions,
    }
}

/// The calls of the open round, in the model's own order. `step_ids` is parallel to
/// `calls`: the step the loop reserved for each, or `""` for a call it rejected
/// before dispatch (which took no step).
pub(crate) fn fold_calls<'a>(
    calls: &[pb::ToolCall],
    step_ids: impl IntoIterator<Item = &'a str>,
) -> Vec<FoldCall> {
    calls
        .iter()
        .zip(step_ids)
        .map(|(call, step_id)| FoldCall {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments_json: call.arguments_json.clone(),
            step_id: step_id.to_owned(),
        })
        .collect()
}

/// The outcomes of the open round that already exist, matched to their calls.
///
/// `results` holds one `(output, error)` per call **except** the paused one, in the
/// model's order (exactly how the loop's outcome list is built); `paused` is the
/// index of the call the loop is paused on, if any, which has no outcome and is
/// excluded. Each entry carries its call's id, and its step id (empty for a
/// rejected call).
pub(crate) fn fold_completed<'a>(
    calls: &[FoldCall],
    paused: Option<usize>,
    results: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<FoldCompleted> {
    let mut results = results.into_iter();
    let mut completed = Vec::new();
    for (index, call) in calls.iter().enumerate() {
        if paused == Some(index) {
            continue;
        }
        let Some((output, error)) = results.next() else {
            break;
        };
        completed.push(FoldCompleted {
            step_id: call.step_id.clone(),
            output: output.to_owned(),
            error: error.to_owned(),
            call_id: call.id.clone(),
        });
    }
    completed
}

/// The outcomes of the calls the loop refused **before dispatch**, matched to their
/// calls: the only calls of a round that already have an outcome when the pre_effect
/// fold is saved.
///
/// `results` is parallel to `calls`: `Some((output, error))` for a call the loop
/// rejected (not offered, truncated, over the autonomy rung, a leaf role, ...), `None`
/// for a call that is still to be dispatched. Each entry carries its call's id and its
/// call's own step id: empty for a call refused before dispatch (it took no step), and,
/// in a round an in-place resume restored, the step a sibling that already ran kept (so
/// the fold records it as one that ran, not as one that was refused). A refusal is saved,
/// never re-derived on resume, because some of them (a truncated final call) depend on
/// state the fold does not carry. A call with an empty `step_id` is therefore resolved
/// here, and a resume never dispatches it.
pub(crate) fn fold_rejected<'a>(
    calls: &[FoldCall],
    results: impl IntoIterator<Item = Option<(&'a str, &'a str)>>,
) -> Vec<FoldCompleted> {
    calls
        .iter()
        .zip(results)
        .filter_map(|(call, result)| {
            let (output, error) = result?;
            Some(FoldCompleted {
                step_id: call.step_id.clone(),
                output: output.to_owned(),
                error: error.to_owned(),
                call_id: call.id.clone(),
            })
        })
        .collect()
}

/// What a failed fit says: the closed reason a content refusal carries.
fn too_big(bytes: &[u8]) -> bool {
    bytes.len() > SAVE_BUDGET_BYTES
}

/// Scrub, gate and serialize a fold for saving.
///
/// `scrub_fold` runs per field and `LoopFoldV1::to_bytes` (the credential gate and
/// the 4 MiB cap) is the last check. If scrubbing had to withhold any piece of the
/// conversation (a message, the open round's text or a call's identity or outcome: only
/// call arguments are exempt, they are withheld by design and an empty-argument call is
/// never redispatched) the fold is **refused** as a credential, which the writer turns
/// into a tombstone: a fold with a stub in the goal, a tool result or a system message
/// would resume on a damaged conversation and nothing in it would say so. A fold over
/// [`SAVE_BUDGET_BYTES`] has its stale
/// tool results cleared **once** (the same tier-1 compaction the loop applies to its
/// own history) and is serialized again; if it is still over, it is refused, never
/// truncated: a truncated fold reads as a complete one.
pub(crate) fn fit(mut fold: LoopFoldV1) -> Result<Vec<u8>, SaveError> {
    if crate::scrub::scrub_fold(&mut fold).conversation_withheld() {
        return Err(SaveError::ContentRefused {
            reason: "credential",
        });
    }
    match fold.to_bytes() {
        Ok(bytes) if !too_big(&bytes) => return Ok(bytes),
        Ok(_) | Err(lf::FoldRefusal::TooLarge) => {}
        Err(lf::FoldRefusal::ContainsCredential) => {
            return Err(SaveError::ContentRefused {
                reason: "credential",
            });
        }
        Err(_) => {
            return Err(SaveError::ContentRefused {
                reason: "malformed",
            })
        }
    }
    // Over budget: clear the stale tool results once, on the fold's own copy.
    let mut messages: Vec<pb::ChatMessage> = fold
        .messages
        .iter()
        .map(|message| pb::ChatMessage {
            role: message.role.clone(),
            content: message.content.clone(),
            name: message.name.clone(),
            compaction_summary: message.compaction_summary.clone(),
        })
        .collect();
    crate::compaction_budget::clear_stale_tool_results(&mut messages);
    fold.messages = fold_messages(&messages);
    match fold.to_bytes() {
        Ok(bytes) if !too_big(&bytes) => Ok(bytes),
        _ => Err(SaveError::ContentRefused { reason: "oversize" }),
    }
}

/// The header of a fold, kept so a tombstone can be written after the fold itself
/// was consumed by [`fit`].
#[derive(Clone)]
struct Header {
    run_id: String,
    segment_no: u32,
    counters: (u32, u32, u32),
    authority: Authority,
}

impl Header {
    fn of(fold: &LoopFoldV1) -> Self {
        Self {
            run_id: fold.run_id.clone(),
            segment_no: fold.segment_no,
            counters: (fold.rounds_executed, fold.delegated_rounds, fold.step_seq),
            authority: fold.authority.clone(),
        }
    }

    fn tombstone(&self, reason: &str) -> LoopFoldV1 {
        LoopFoldV1::tombstone(
            &self.run_id,
            self.segment_no,
            self.counters,
            self.authority.clone(),
            reason,
        )
    }

    /// The serialized tombstone and the authority digest to save it under. Built from
    /// the header alone, so nothing of the fold that was refused (its conversation, open
    /// round or pause) can reach it.
    fn tombstone_bytes(&self, reason: &str) -> Result<(Vec<u8>, String), SaveError> {
        let tombstone = self.tombstone(reason);
        let digest = tombstone.authority.digest();
        match tombstone.to_bytes() {
            Ok(bytes) => Ok((bytes, digest)),
            Err(_) => Err(SaveError::ContentRefused {
                reason: "malformed",
            }),
        }
    }
}

/// The metrics facade (no recorder is installed in execution-core today, so these
/// are no-ops until one is; the `elapsed_ms` log field is the latency evidence).
fn record_metrics(kind: SaveKind, outcome: &'static str, bytes: usize, elapsed: Duration) {
    metrics::counter!(
        "execution_core_loop_fold_saves_total",
        "kind" => kind.as_str(),
        "outcome" => outcome
    )
    .increment(1);
    metrics::histogram!("execution_core_loop_fold_save_seconds", "kind" => kind.as_str())
        .record(elapsed.as_secs_f64());
    #[allow(clippy::cast_precision_loss)]
    metrics::histogram!("execution_core_loop_fold_bytes", "kind" => kind.as_str())
        .record(bytes as f64);
}

/// Save one fold through `SaveLoopCheckpoint`.
///
/// `Ok` is the seq session-core stored it under. The failure is classified
/// ([`classify_status`]); a timeout is an ambiguous `Unavailable`. One structured,
/// content-free event per call: ids, kind, seqs, byte count, elapsed time and a
/// closed outcome token, never the fold, a message, or a status message.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_rpc(
    channel: &Channel,
    bearer: &str,
    run_id: &str,
    expected_seq: u64,
    kind: SaveKind,
    fold: &[u8],
    authority_digest: &str,
    pause_ref: &str,
    timeout: Duration,
) -> Result<u64, SaveError> {
    let mut request = tonic::Request::new(pb::SaveLoopCheckpointRequest {
        run_id: run_id.to_owned(),
        expected_seq,
        kind: kind.as_str().to_owned(),
        fold: fold.to_vec(),
        authority_digest: authority_digest.to_owned(),
        pause_ref: pause_ref.to_owned(),
        park_reason: String::new(),
        park_until: None,
    });
    let Ok(header) = format!("Bearer {bearer}").parse() else {
        return Err(SaveError::Unavailable { ambiguous: false });
    };
    request.metadata_mut().insert("authorization", header);

    let started = Instant::now();
    let result = tokio::time::timeout(
        timeout,
        SessionCoreClient::new(channel.clone()).save_loop_checkpoint(request),
    )
    .await;
    let elapsed = started.elapsed();
    let outcome: Result<u64, SaveError> = match result {
        Err(_elapsed) => Err(SaveError::Unavailable { ambiguous: true }),
        Ok(Err(status)) => Err(classify_status(&status)),
        Ok(Ok(response)) => Ok(response.into_inner().seq),
    };
    let token = outcome
        .as_ref()
        .map_or_else(|error| error.as_str(), |_| "saved");
    info!(
        target: "loop_fold",
        run_id = %run_id,
        kind = kind.as_str(),
        expected_seq,
        seq = ?outcome.as_ref().ok(),
        bytes = fold.len(),
        elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        outcome = token,
        cause = outcome.as_ref().err().map_or("", |error| error.cause()),
        "loop fold save"
    );
    record_metrics(kind, token, fold.len(), elapsed);
    outcome
}

/// Close a run's fold (`CloseLoopCheckpoint`), best effort: used when a pause fold
/// was saved and its approval could not be created, so no orphan paused fold stays
/// behind. A failure is logged with a closed token and changes nothing.
pub(crate) async fn close_rpc(
    channel: &Channel,
    bearer: &str,
    run_id: &str,
    seq: u64,
    reason: &'static str,
    timeout: Duration,
) {
    let mut request = tonic::Request::new(pb::CloseLoopCheckpointRequest {
        run_id: run_id.to_owned(),
        seq,
        reason: reason.to_owned(),
    });
    let Ok(header) = format!("Bearer {bearer}").parse() else {
        return;
    };
    request.metadata_mut().insert("authorization", header);
    let result = tokio::time::timeout(
        timeout,
        SessionCoreClient::new(channel.clone()).close_loop_checkpoint(request),
    )
    .await;
    let outcome = match result {
        Err(_) => "timeout",
        Ok(Err(status)) => classify_status(&status).as_str(),
        Ok(Ok(_)) => "closed",
    };
    info!(
        target: "loop_fold",
        run_id = %run_id,
        seq,
        reason,
        outcome,
        "loop fold close"
    );
}

/// One park save, as the RPC sees it ([`FoldWriter::save_park`]).
struct ParkSave {
    expected_seq: u64,
    bytes: Vec<u8>,
    authority_digest: String,
    pause_ref: String,
    reason: ParkReason,
    park_until: DateTime<Utc>,
}

/// Save one park fold through `SaveLoopCheckpoint` (kind `park`): [`save_rpc`]'s shape with
/// the park's reason and due time, and the one answer a park needs that no other save does.
///
/// `Ok((seq, accepted))`: `accepted` is session-core's `park_accepted`, true only when a
/// park-aware store durably accepted this save under its park rules. A store that predates
/// the field answers false, so a park it merely stored is never mistaken for a taken one.
/// One structured, content-free event per call, like [`save_rpc`].
async fn park_rpc(
    channel: &Channel,
    bearer: &str,
    run_id: &str,
    save: &ParkSave,
) -> Result<(u64, bool), SaveError> {
    let mut request = tonic::Request::new(pb::SaveLoopCheckpointRequest {
        run_id: run_id.to_owned(),
        expected_seq: save.expected_seq,
        kind: SaveKind::Park.as_str().to_owned(),
        fold: save.bytes.clone(),
        authority_digest: save.authority_digest.clone(),
        pause_ref: save.pause_ref.clone(),
        park_reason: save.reason.as_str().to_owned(),
        park_until: Some(prost_types::Timestamp::from(std::time::SystemTime::from(
            save.park_until,
        ))),
    });
    let Ok(header) = format!("Bearer {bearer}").parse() else {
        return Err(SaveError::Unavailable { ambiguous: false });
    };
    request.metadata_mut().insert("authorization", header);

    let started = Instant::now();
    let result = tokio::time::timeout(
        SaveKind::Park.timeout(),
        SessionCoreClient::new(channel.clone()).save_loop_checkpoint(request),
    )
    .await;
    let elapsed = started.elapsed();
    let outcome: Result<(u64, bool), SaveError> = match result {
        Err(_elapsed) => Err(SaveError::Unavailable { ambiguous: true }),
        Ok(Err(status)) => Err(classify_status(&status)),
        Ok(Ok(response)) => {
            let response = response.into_inner();
            Ok((response.seq, response.park_accepted))
        }
    };
    let token = match &outcome {
        Ok((_, true)) => "saved",
        Ok((_, false)) => "not_accepted",
        Err(error) => error.as_str(),
    };
    info!(
        target: "loop_fold",
        run_id = %run_id,
        kind = SaveKind::Park.as_str(),
        expected_seq = save.expected_seq,
        seq = ?outcome.as_ref().ok().map(|(seq, _)| seq),
        park_reason = save.reason.as_str(),
        bytes = save.bytes.len(),
        elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        outcome = token,
        cause = outcome.as_ref().err().map_or("", |error| error.cause()),
        "loop fold save"
    );
    record_metrics(SaveKind::Park, token, save.bytes.len(), elapsed);
    outcome
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Active,
    Off(OffReason),
    Failed(SaveError),
}

/// What a pre-effect save lets the loop do with the round's effectful calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreEffect {
    /// The fold is durable: dispatch.
    Saved,
    /// No resumable fold exists and none can replay the effect (a tombstone was
    /// saved, or the store is explicitly off): dispatch.
    Degraded,
    /// The state could not be saved ahead of the effect: hold the effectful calls.
    Hold,
}

/// One fence save, as the RPC sees it ([`FoldWriter::save_fence`]).
struct FenceSave {
    expected_seq: u64,
    kind: SaveKind,
    bytes: Vec<u8>,
    authority_digest: String,
    pause_ref: String,
}

/// The writer of one run's fold. A plain `&mut` local of `run_rounds`: saves happen
/// strictly in sequence (between dispatch phases and at round end), so `expected_seq`
/// needs no lock.
pub(crate) struct FoldWriter<'a> {
    channel: &'a Channel,
    bearer: Option<&'a str>,
    run_id: &'a str,
    tracker: SeqTracker,
    /// The segment this writer's folds are written by: 1 for a run's first loop. A
    /// resumed loop (WS03 S7b) is seeded with its own.
    segment_no: u32,
    state: State,
}

impl<'a> FoldWriter<'a> {
    /// A writer for a loop that resumes a claimed fold: it expects the claimed seq next
    /// and writes under `segment_no`. It goes through the same [`WriterGate`] as
    /// [`FoldWriter::new`] (switch, ledger, zero-retention, waived, Space, delegated): it
    /// is OFF with the gate's reason when the gate refuses and when there is no bearer,
    /// so a caller that forgets to gate the run cannot write folds for a ZDR or waived
    /// run. (`gate.resumed` is the stage-1 segment flag and an in-place resume is never one,
    /// so this constructor forces it false itself: the decision a caller makes before it
    /// claims, `resume::in_place_writer_off`, and this writer's own are one decision by
    /// construction, whatever the caller put in `resumed`.)
    /// Pure: it makes no RPC.
    pub(crate) fn seeded(
        channel: &'a Channel,
        bearer: Option<&'a str>,
        run_id: &'a str,
        gate: WriterGate,
        expected_seq: u64,
        segment_no: u32,
    ) -> Self {
        let gate = WriterGate {
            resumed: false,
            ..gate
        };
        let mut writer = Self::new(channel, bearer, run_id, gate);
        writer.tracker = SeqTracker::seeded(expected_seq);
        writer.segment_no = segment_no;
        writer
    }

    /// A writer for one loop. Pure: it makes no RPC, whatever the gate says.
    pub(crate) fn new(
        channel: &'a Channel,
        bearer: Option<&'a str>,
        run_id: &'a str,
        gate: WriterGate,
    ) -> Self {
        let state = match gate.decide() {
            Some(reason) => State::Off(reason),
            None if bearer.is_none() => State::Off(OffReason::NoBearer),
            None => State::Active,
        };
        // A switch that is on but cannot work is a configuration problem worth one
        // line per run; every other reason is the run's ordinary posture.
        if let State::Off(reason @ (OffReason::LedgerOff | OffReason::NoBearer)) = state {
            if gate.switch && gate.depth == 0 && !gate.resumed {
                warn!(
                    target: "loop_fold",
                    run_id = %run_id,
                    reason = reason.as_str(),
                    "the loop fold writer is on but cannot run for this run; no fold is saved"
                );
            }
        }
        Self {
            channel,
            bearer,
            run_id,
            tracker: SeqTracker::default(),
            segment_no: 1,
            state,
        }
    }

    /// The segment number the folds of this writer carry. (Not named `segment_no`: the
    /// source contract forbids the executor loop from reading a `.segment_no`, which is
    /// the client-supplied resume field; this is the writer's own.)
    pub(crate) const fn segment(&self) -> u32 {
        self.segment_no
    }

    /// The writer is saving folds.
    pub(crate) fn is_active(&self) -> bool {
        self.state == State::Active
    }

    /// The writer has failed for good: nothing more will be saved in this run, so a
    /// call it holds back is not held for a moment but for the rest of the run.
    pub(crate) fn is_failed(&self) -> bool {
        matches!(self.state, State::Failed(_))
    }

    /// The writer lost its fence: a save came back stale or closed, so another writer holds the
    /// run's fold, or the fold is closed for good. Only a parkable loop acts on it (it stops before
    /// anything more is inferred, appended or dispatched). A refusal for good is not a lost fence:
    /// nothing else holds the fold, and the loop keeps the behaviour it always had.
    pub(crate) fn lost_fence(&self) -> bool {
        matches!(self.state, State::Failed(SaveError::Stale | SaveError::Closed))
    }

    /// The writer is saving folds or has failed. A failed writer holds effects for
    /// the rest of the run, so the effect gate must still ask it.
    pub(crate) fn engaged(&self) -> bool {
        !matches!(self.state, State::Off(_))
    }

    /// Why the writer is off, if it is.
    #[cfg(test)] // read by the tests; a log line's reason is at `new`
    pub(crate) fn off_reason(&self) -> Option<OffReason> {
        match self.state {
            State::Off(reason) => Some(reason),
            _ => None,
        }
    }

    /// The seq the next save will expect. Read by the tests (the fence is written at the seq
    /// the claim returned) and by a failed park (`park::try_park` closes the row a park save
    /// that may have committed would sit at, `expected + 1`).
    pub(crate) fn expected_seq(&self) -> u64 {
        self.tracker.expected()
    }

    /// A save failed: move the state machine. Stale and closed stop the writer
    /// (a second writer exists, or nothing more may be written); a disabled or
    /// non-persistent store turns it off, which is switch-off behaviour. A store that
    /// refuses the run for a reason that will not change (not found, denied, a
    /// session-core without the RPC, no key) fails the writer at once, with one
    /// content-free line that names the cause: asking again would get the same answer
    /// and hold every effectful call again.
    fn apply(&mut self, error: SaveError) {
        match error {
            SaveError::Refused(by) => {
                warn!(
                    target: "loop_fold",
                    run_id = %self.run_id,
                    cause = by.as_str(),
                    "the loop fold store refused this run for good; no fold is saved and every \
                     effectful call is held back for the rest of the run"
                );
                self.state = State::Failed(error);
            }
            SaveError::Stale | SaveError::Closed => self.state = State::Failed(error),
            SaveError::StoreOff => self.state = State::Off(OffReason::StoreDisabled),
            SaveError::NotPersistent => self.state = State::Off(OffReason::RunNotPersistent),
            SaveError::ContentRefused { .. } | SaveError::Unavailable { .. } => {}
        }
    }

    /// Save already-serialized bytes with the tracker's `expected_seq`, retrying
    /// once with `expected + 1` if a stale answer follows an ambiguous failure.
    async fn save_bytes(
        &mut self,
        kind: SaveKind,
        bytes: &[u8],
        authority_digest: &str,
        pause_ref: &str,
    ) -> Result<u64, SaveError> {
        let Some(bearer) = self.bearer else {
            return Err(SaveError::Unavailable { ambiguous: false });
        };
        let mut result = save_rpc(
            self.channel,
            bearer,
            self.run_id,
            self.tracker.expected(),
            kind,
            bytes,
            authority_digest,
            pause_ref,
            kind.timeout(),
        )
        .await;
        if result == Err(SaveError::Stale) {
            if let Some(probe) = self.tracker.probe() {
                result = save_rpc(
                    self.channel,
                    bearer,
                    self.run_id,
                    probe,
                    kind,
                    bytes,
                    authority_digest,
                    pause_ref,
                    kind.timeout(),
                )
                .await;
            }
        }
        match result {
            Ok(seq) => self.tracker.on_saved(seq),
            Err(error) => {
                self.tracker.on_failed(&error);
                self.apply(error);
            }
        }
        result
    }

    /// Scrub, gate and save a fold.
    async fn save_fold(
        &mut self,
        kind: SaveKind,
        fold: LoopFoldV1,
        pause_ref: &str,
    ) -> Result<u64, SaveError> {
        let digest = fold.authority.digest();
        let bytes = match fit(fold) {
            Ok(bytes) => bytes,
            Err(error) => {
                info!(
                    target: "loop_fold",
                    run_id = %self.run_id,
                    kind = kind.as_str(),
                    outcome = error.as_str(),
                    reason = error.cause(),
                    "loop fold not saved"
                );
                record_metrics(kind, error.as_str(), 0, Duration::ZERO);
                return Err(error);
            }
        };
        self.save_bytes(kind, &bytes, &digest, pause_ref).await
    }

    /// Replace the stored fold with a tombstone that says "do not resume from
    /// here". Best effort and kind `round`, so the next ordinary save supersedes it.
    async fn save_tombstone(&mut self, header: &Header, reason: &str) -> Result<u64, SaveError> {
        self.save_tombstone_as(header, reason, SaveKind::Round, "")
            .await
    }

    /// A tombstone saved under `kind` and `pause_ref`: the pause fold's replacement
    /// uses kind `pause` and the pause's own reference, so the stored row is the pause's
    /// and not a resumable fold of an earlier save.
    async fn save_tombstone_as(
        &mut self,
        header: &Header,
        reason: &str,
        kind: SaveKind,
        pause_ref: &str,
    ) -> Result<u64, SaveError> {
        let (bytes, digest) = header.tombstone_bytes(reason)?;
        self.save_bytes(kind, &bytes, &digest, pause_ref).await
    }

    /// The end of a round that ran tool calls. Best effort: it never errors, never
    /// holds the loop, and a failure is skipped. A content refusal tries a
    /// tombstone, so a crash cannot resume from a fold older than the round.
    pub(crate) async fn save_round(&mut self, fold: LoopFoldV1) {
        if !self.is_active() {
            return;
        }
        let header = Header::of(&fold);
        if let Err(SaveError::ContentRefused { reason }) =
            self.save_fold(SaveKind::Round, fold, "").await
        {
            if self.is_active() {
                let _ = self.save_tombstone(&header, reason).await;
            }
        }
    }

    /// Before a round dispatches an effectful call. Fail closed: only
    /// [`PreEffect::Saved`] and [`PreEffect::Degraded`] let the effect start.
    ///
    /// * saved: durable, dispatch;
    /// * unavailable: hold this round's effectful calls, stay active, the next
    ///   round tries again;
    /// * content refused: a tombstone replaces the stored fold (nothing resumable
    ///   can replay the effect), then dispatch; if the tombstone also fails, hold;
    /// * stale or closed, or refused for good by the store: the writer is failed,
    ///   and holds effects for the rest of the run;
    /// * the store off or the run not persistent: the writer turns off and the run
    ///   behaves exactly as with the switch off.
    pub(crate) async fn save_pre_effect(&mut self, fold: LoopFoldV1) -> PreEffect {
        match self.state {
            State::Off(_) => return PreEffect::Degraded,
            State::Failed(_) => return PreEffect::Hold,
            State::Active => {}
        }
        let header = Header::of(&fold);
        match self.save_fold(SaveKind::PreEffect, fold, "").await {
            Ok(_) => PreEffect::Saved,
            Err(SaveError::ContentRefused { reason }) => {
                if self.is_active() && self.save_tombstone(&header, reason).await.is_ok() {
                    PreEffect::Degraded
                } else {
                    PreEffect::Hold
                }
            }
            Err(SaveError::StoreOff | SaveError::NotPersistent) => PreEffect::Degraded,
            Err(
                SaveError::Stale
                | SaveError::Closed
                | SaveError::Refused(_)
                | SaveError::Unavailable { .. },
            ) => PreEffect::Hold,
        }
    }

    /// The fence of an in-place resume: ONE `pre_effect` save at the claimed seq, before any
    /// call of the resumed round is reserved or dispatched.
    ///
    /// Two claimants can hold the same claimed seq (a claim whose lease lapsed, or a replay of
    /// the same request), and the claim does not bump the seq. The seq compare-and-set is
    /// therefore the fence: of two saves at one seq exactly one wins, and the loser gets
    /// [`SaveError::Stale`] and must dispatch nothing. So this save is deliberately not a
    /// round save:
    ///
    /// * it makes exactly one RPC, with no probe: the retry a stale answer gets after an
    ///   ambiguous failure would overwrite the claimant that won;
    /// * a content refusal is an error, never a tombstone and never a degrade: without a
    ///   durable fence nothing stops a second claimant, so the resume dispatches nothing;
    /// * a writer that is not active (off, or failed) makes no call and fails: a resume whose
    ///   writer is `Off(NoBearer)` or `Failed` has no fence to write.
    ///
    /// The seq moves to the saved one on success. Any failure moves the writer as any save's
    /// does (a stale or closed answer stops it for good).
    ///
    /// # Errors
    ///
    /// The [`SaveError`] of the failed save, or `Unavailable { ambiguous: false }` (no RPC
    /// was made) when the writer is not active.
    pub(crate) async fn save_fence(&mut self, fold: LoopFoldV1) -> Result<u64, SaveError> {
        let (channel, run_id) = (self.channel, self.run_id);
        let Some(bearer) = self.bearer else {
            return Err(SaveError::Unavailable { ambiguous: false });
        };
        self.fence_with(fold, |save| async move {
            save_rpc(
                channel,
                bearer,
                run_id,
                save.expected_seq,
                save.kind,
                &save.bytes,
                &save.authority_digest,
                &save.pause_ref,
                GATED_SAVE_TIMEOUT,
            )
            .await
        })
        .await
    }

    /// [`FoldWriter::save_fence`] with the RPC passed in, so the one-call rule and the
    /// failure handling are testable without a gRPC server.
    async fn fence_with<F, Fut>(&mut self, fold: LoopFoldV1, rpc: F) -> Result<u64, SaveError>
    where
        F: FnOnce(FenceSave) -> Fut,
        Fut: std::future::Future<Output = Result<u64, SaveError>>,
    {
        if !self.is_active() {
            return Err(SaveError::Unavailable { ambiguous: false });
        }
        let authority_digest = fold.authority.digest();
        let bytes = match fit(fold) {
            Ok(bytes) => bytes,
            Err(error) => {
                info!(
                    target: "loop_fold",
                    run_id = %self.run_id,
                    kind = SaveKind::PreEffect.as_str(),
                    outcome = error.as_str(),
                    reason = error.cause(),
                    "loop fold not saved"
                );
                record_metrics(SaveKind::PreEffect, error.as_str(), 0, Duration::ZERO);
                return Err(error);
            }
        };
        let result = rpc(FenceSave {
            expected_seq: self.tracker.expected(),
            kind: SaveKind::PreEffect,
            bytes,
            authority_digest,
            pause_ref: String::new(),
        })
        .await;
        match result {
            Ok(seq) => self.tracker.on_saved(seq),
            Err(error) => {
                self.tracker.on_failed(&error);
                self.apply(error);
            }
        }
        result
    }

    /// The park (WS03 S8-A): ONE `park` save at the expected seq, at a round boundary,
    /// naming why the run waits and when a resume is first due. `fold` carries the park's
    /// own `pending` (kind park, its `pause_ref`) and `park_until` is the earliest resume.
    ///
    /// Like the fence, and unlike a round save, it is deliberately plain:
    ///
    /// * it makes exactly one RPC, with no probe: a park whose save may have committed is
    ///   not retried at `expected + 1`, the caller closes that row instead
    ///   (`park::try_park`);
    /// * a content refusal is an error, never a tombstone: a tombstone saved as the park
    ///   would be a park nothing can resume;
    /// * a writer that is not active (off, or failed) makes no call and fails;
    /// * the seq moves only when the store accepted the park. A save it merely stored
    ///   ([`ParkSaveError::NotAccepted`]) leaves the writer as it was.
    ///
    /// # Errors
    ///
    /// [`ParkSaveError::Save`] with the failed save's [`SaveError`], or
    /// `Unavailable { ambiguous: false }` (no RPC was made) when the writer is not active or
    /// the fold does not name a park; [`ParkSaveError::NotAccepted`] when the store stored
    /// the fold without accepting it as a park.
    pub(crate) async fn save_park(
        &mut self,
        fold: LoopFoldV1,
        reason: ParkReason,
        park_until: DateTime<Utc>,
    ) -> Result<u64, ParkSaveError> {
        let (channel, run_id) = (self.channel, self.run_id);
        let Some(bearer) = self.bearer else {
            return Err(ParkSaveError::Save(SaveError::Unavailable {
                ambiguous: false,
            }));
        };
        self.park_with(fold, reason, park_until, |save| async move {
            park_rpc(channel, bearer, run_id, &save).await
        })
        .await
    }

    /// [`FoldWriter::save_park`] with the RPC passed in, so the one-call rule and the
    /// failure handling are testable without a gRPC server.
    async fn park_with<F, Fut>(
        &mut self,
        fold: LoopFoldV1,
        reason: ParkReason,
        park_until: DateTime<Utc>,
        rpc: F,
    ) -> Result<u64, ParkSaveError>
    where
        F: FnOnce(ParkSave) -> Fut,
        Fut: std::future::Future<Output = Result<(u64, bool), SaveError>>,
    {
        if !self.is_active() {
            return Err(ParkSaveError::Save(SaveError::Unavailable {
                ambiguous: false,
            }));
        }
        // A park fold names its own reference: the claim of a park matches on it.
        let pause_ref = match fold.pending.as_ref() {
            Some(pending) if pending.kind == PauseKind::Park && !pending.pause_ref.is_empty() => {
                pending.pause_ref.clone()
            }
            _ => {
                return Err(ParkSaveError::Save(SaveError::ContentRefused {
                    reason: "malformed",
                }))
            }
        };
        let authority_digest = fold.authority.digest();
        let bytes = match fit(fold) {
            Ok(bytes) => bytes,
            Err(error) => {
                info!(
                    target: "loop_fold",
                    run_id = %self.run_id,
                    kind = SaveKind::Park.as_str(),
                    outcome = error.as_str(),
                    reason = error.cause(),
                    "loop fold not saved"
                );
                record_metrics(SaveKind::Park, error.as_str(), 0, Duration::ZERO);
                return Err(ParkSaveError::Save(error));
            }
        };
        let result = rpc(ParkSave {
            expected_seq: self.tracker.expected(),
            bytes,
            authority_digest,
            pause_ref,
            reason,
            park_until,
        })
        .await;
        match result {
            Ok((seq, true)) => {
                self.tracker.on_saved(seq);
                Ok(seq)
            }
            Ok((seq, false)) => Err(ParkSaveError::NotAccepted { seq }),
            Err(error) => {
                self.tracker.on_failed(&error);
                self.apply(error);
                Err(ParkSaveError::Save(error))
            }
        }
    }

    /// The pause fold, ready to be saved just before the approval is created. `None`
    /// unless the writer is active.
    pub(crate) fn pause_fold(&mut self, fold: LoopFoldV1) -> Option<PauseFold<'_, 'a>> {
        if !self.is_active() {
            return None;
        }
        Some(PauseFold {
            writer: self,
            fold,
            saved_seq: None,
        })
    }
}

/// Count calls held back, for the metric. The log line for each is the caller's.
pub(crate) fn note_effects_held(count: usize) {
    metrics::counter!("execution_core_loop_fold_effects_held_total")
        .increment(u64::try_from(count).unwrap_or(u64::MAX));
}

/// What saving the pause fold came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PauseSaved {
    /// Durable under this seq: the approval may be created and continued in place.
    Saved { seq: u64 },
    /// The fold's content was refused (credential-shaped, too large, rejected as
    /// invalid), so a tombstone was saved as the pause fold instead, under the same
    /// pause reference: the row behind the approval is not a resumable fold of an
    /// earlier save. Never resumable; `/continue` falls back to stage 1 at claim time.
    Tombstoned { seq: u64 },
    /// Not saved. The approval is still created (a pause is never refused for the
    /// fold's sake) and `/continue` falls back to stage 1 at claim time.
    NotSaved,
}

/// A pause's fold, built from the loop's locals but not yet bound to its approval.
pub(crate) struct PauseFold<'w, 'a> {
    writer: &'w mut FoldWriter<'a>,
    fold: LoopFoldV1,
    saved_seq: Option<u64>,
}

impl PauseFold<'_, '_> {
    /// Save the fold for the pause on `step_id`: `pending` names the approval by its
    /// idempotency key `{run_id}:{step_id}`, the same key the approval is created
    /// under, so a resume derives it from the consumed approval's step id.
    ///
    /// A content refusal (credential-shaped, oversize, rejected as invalid) saves a
    /// tombstone as the pause fold instead ([`PauseSaved::Tombstoned`]); if that save
    /// fails too, or any other save error occurs, nothing more is done
    /// ([`PauseSaved::NotSaved`]) and the pause goes on.
    pub(crate) async fn save(
        &mut self,
        step_id: &str,
        tool_name: &str,
        action_fingerprint: &str,
    ) -> PauseSaved {
        self.fold.pending = Some(PendingPause {
            kind: PauseKind::Approval,
            pause_ref: format!("{}:{step_id}", self.writer.run_id),
            step_id: step_id.to_owned(),
            call: tool_name.to_owned(),
            action_fingerprint: action_fingerprint.to_owned(),
        });
        let pause_ref = format!("{}:{step_id}", self.writer.run_id);
        let header = Header::of(&self.fold);
        match self
            .writer
            .save_fold(SaveKind::Pause, self.fold.clone(), &pause_ref)
            .await
        {
            Ok(seq) => {
                self.saved_seq = Some(seq);
                PauseSaved::Saved { seq }
            }
            // The earlier pre_effect fold is still the stored one (resumable, with the
            // open round), and the pause is about to wait on an approval: replace it by
            // a tombstone saved AS the pause fold. Only a content refusal does this; a
            // store failure leaves the row as it was, as before.
            Err(SaveError::ContentRefused { reason }) if self.writer.is_active() => {
                match self
                    .writer
                    .save_tombstone_as(&header, reason, SaveKind::Pause, &pause_ref)
                    .await
                {
                    Ok(seq) => {
                        // Closed again if the approval cannot be created, like any pause fold.
                        self.saved_seq = Some(seq);
                        info!(
                            target: "loop_fold",
                            run_id = %self.writer.run_id,
                            reason,
                            "the pause fold was refused as content; a tombstone was saved in its place"
                        );
                        PauseSaved::Tombstoned { seq }
                    }
                    Err(error) => {
                        // The pause goes on: the approval is raised either way.
                        warn!(
                            target: "loop_fold",
                            run_id = %self.writer.run_id,
                            reason,
                            outcome = error.as_str(),
                            cause = error.cause(),
                            "the pause fold was refused as content and its tombstone could not be saved"
                        );
                        PauseSaved::NotSaved
                    }
                }
            }
            Err(_) => PauseSaved::NotSaved,
        }
    }

    /// The approval could not be created after the fold was saved: close the fold,
    /// so no orphan paused row stays behind. Best effort; a no-op if nothing saved.
    pub(crate) async fn close_after_failed_approval(&mut self) {
        let (Some(seq), Some(bearer)) = (self.saved_seq.take(), self.writer.bearer) else {
            return;
        };
        close_rpc(
            self.writer.channel,
            bearer,
            self.writer.run_id,
            seq,
            "approval_not_created",
            GATED_SAVE_TIMEOUT,
        )
        .await;
        // A closed fold refuses every later save of the run.
        self.writer.state = State::Failed(SaveError::Closed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_ROUND: &str = include_str!(
        "../../../../crates/mp-contracts/tests/fixtures/loop-fold-v1-writer-round.json"
    );
    const GOLDEN_PRE_EFFECT: &str = include_str!(
        "../../../../crates/mp-contracts/tests/fixtures/loop-fold-v1-writer-pre-effect.json"
    );
    const GOLDEN_PAUSE: &str = include_str!(
        "../../../../crates/mp-contracts/tests/fixtures/loop-fold-v1-writer-pause.json"
    );

    fn all_good() -> WriterGate {
        WriterGate {
            switch: true,
            ledger: true,
            zdr: false,
            waived: false,
            space: false,
            depth: 0,
            resumed: false,
        }
    }

    #[test]
    fn the_writer_is_off_unless_switch_ledger_and_a_persistent_root_run_allow_it() {
        assert_eq!(
            all_good().decide(),
            None,
            "the control: the all-good gate writes"
        );
        let cases = [
            (
                WriterGate {
                    switch: false,
                    ..all_good()
                },
                OffReason::SwitchOff,
            ),
            (
                WriterGate {
                    ledger: false,
                    ..all_good()
                },
                OffReason::LedgerOff,
            ),
            (
                WriterGate {
                    zdr: true,
                    ..all_good()
                },
                OffReason::Zdr,
            ),
            (
                WriterGate {
                    waived: true,
                    ..all_good()
                },
                OffReason::Waived,
            ),
            (
                WriterGate {
                    space: true,
                    ..all_good()
                },
                OffReason::Space,
            ),
            (
                WriterGate {
                    depth: 1,
                    ..all_good()
                },
                OffReason::Delegated,
            ),
            (
                WriterGate {
                    resumed: true,
                    ..all_good()
                },
                OffReason::ResumedSegment,
            ),
        ];
        for (gate, reason) in cases {
            assert_eq!(gate.decide(), Some(reason), "{gate:?}");
        }
        // Every refusal reason has its own closed token.
        let tokens: BTreeSet<&str> = [
            OffReason::SwitchOff,
            OffReason::LedgerOff,
            OffReason::Zdr,
            OffReason::Waived,
            OffReason::Space,
            OffReason::Delegated,
            OffReason::ResumedSegment,
            OffReason::NoBearer,
            OffReason::StoreDisabled,
            OffReason::RunNotPersistent,
        ]
        .into_iter()
        .map(OffReason::as_str)
        .collect();
        assert_eq!(tokens.len(), 10);
    }

    #[tokio::test]
    async fn a_writer_with_no_bearer_or_a_closed_gate_is_off_and_makes_no_call() {
        // A channel nothing listens on: an RPC from it would be a refused connection,
        // and a writer that is off never tries.
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let off = FoldWriter::new(
            &channel,
            Some("b"),
            "run-1",
            WriterGate {
                switch: false,
                ..all_good()
            },
        );
        assert_eq!(off.off_reason(), Some(OffReason::SwitchOff));
        assert!(!off.is_active() && !off.engaged());
        let no_bearer = FoldWriter::new(&channel, None, "run-1", all_good());
        assert_eq!(no_bearer.off_reason(), Some(OffReason::NoBearer));
        let on = FoldWriter::new(&channel, Some("b"), "run-1", all_good());
        assert!(on.is_active() && on.engaged());
        assert_eq!(on.expected_seq(), 0);
    }

    #[test]
    fn the_switch_is_off_unless_true() {
        for off in [
            None,
            Some(""),
            Some("1"),
            Some("on"),
            Some("yes"),
            Some("maybe"),
        ] {
            assert!(!lf::switch_from(off), "{off:?}");
        }
        assert!(lf::switch_from(Some("true")));
        // The override a test holds wins over the environment, and is released.
        {
            let _on = CheckpointSwitch::set(true);
            assert!(checkpoint_enabled());
        }
        {
            let _off = CheckpointSwitch::set(false);
            assert!(!checkpoint_enabled());
        }
    }

    fn chat(role: &str, content: &str) -> pb::ChatMessage {
        pb::ChatMessage {
            compaction_summary: String::new(),
            role: role.to_owned(),
            content: content.to_owned(),
            name: String::new(),
        }
    }

    fn authority() -> Authority {
        Authority {
            mode: "ask".to_owned(),
            plan_mode: false,
            autonomy_rung: "unspecified".to_owned(),
            space_id: String::new(),
            min_privacy_tier: 0,
            provider_hint: String::new(),
            tool_names: vec!["delete_records".to_owned(), "knowledge_search".to_owned()],
            max_rounds: 4,
        }
    }

    fn golden_tools() -> Vec<pb::ToolDefinition> {
        vec![
            pb::ToolDefinition {
                name: "knowledge_search".to_owned(),
                description: "Search the org knowledge base.".to_owned(),
                parameters_json: r#"{"type":"object","properties":{"query":{"type":"string"}}}"#
                    .to_owned(),
            },
            pb::ToolDefinition {
                name: "delete_records".to_owned(),
                description: "Delete records (destructive).".to_owned(),
                parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
            },
        ]
    }

    fn golden_messages() -> Vec<pb::ChatMessage> {
        vec![
            chat("system", "SYSTEM_PROMPT_GOLDEN"),
            chat("user", "Find the contract and delete the stale records"),
            chat("assistant", "Searching."),
            chat(
                "user",
                "Tool results for your previous request: knowledge_search found 1 document",
            ),
        ]
    }

    fn golden_digest() -> String {
        prompt_digest_of("SYSTEM_PROMPT_GOLDEN", &golden_tools())
    }

    fn golden_retrievals() -> BTreeSet<String> {
        // The real signature the phase-1 pre-pass produces (`route|top_k|normalised
        // query`), of a search an earlier round already ran. The open round's own search
        // ("stale records") is NOT in the set the open-round folds carry.
        BTreeSet::from(["hybrid|5|find the contract".to_owned()])
    }

    fn value(fold: &LoopFoldV1) -> serde_json::Value {
        serde_json::to_value(fold).expect("serialize")
    }

    fn golden_value(golden: &str) -> serde_json::Value {
        serde_json::from_str(golden).expect("golden")
    }

    /// The three save points of the golden scenario, built the way `run_rounds`
    /// builds them from its locals, must equal the checked-in contract.
    #[test]
    fn round_pre_effect_and_pause_folds_populate_the_fields_the_plan_requires() {
        let retrievals = golden_retrievals();
        let messages = golden_messages();
        let digest = golden_digest();
        let snapshot = |rounds_executed, step_seq| Snapshot {
            run_id: "run-golden-w",
            segment_no: 1,
            rounds_executed,
            delegated_rounds: 0,
            step_seq,
            children_started: 0,
            grounded: true,
            compaction_triggered: false,
            attempted_retrievals: &retrievals,
            messages: &messages,
            authority: authority(),
            prompt_digest: &digest,
            effect_ledger: true,
        };

        // round: between rounds, no open round, no pause.
        let round = build_fold(&snapshot(1, 1), None, None);
        assert_eq!(value(&round), golden_value(GOLDEN_ROUND), "the round fold");

        // pre_effect: the model's three calls in order, a rejected one with no step.
        let tool_calls = vec![
            pb::ToolCall {
                id: "call-2".to_owned(),
                name: "knowledge_search".to_owned(),
                arguments_json: r#"{"query":"stale records"}"#.to_owned(),
            },
            pb::ToolCall {
                id: "call-3".to_owned(),
                name: "unknown_tool".to_owned(),
                arguments_json: "{}".to_owned(),
            },
            pb::ToolCall {
                id: "call-4".to_owned(),
                name: "delete_records".to_owned(),
                arguments_json: r#"{"table":"stale"}"#.to_owned(),
            },
        ];
        let calls = fold_calls(&tool_calls, ["tool_2_call-2", "", "tool_3_call-4"]);
        let open = RoundInFlight {
            assistant_content: "Looking and deleting.".to_owned(),
            calls: calls.clone(),
            // The refused call already has its outcome; the two dispatchable calls
            // have none.
            completed: fold_rejected(
                &calls,
                [
                    None,
                    Some((
                        "",
                        "tool 'unknown_tool' is not in this agent's allowed scope",
                    )),
                    None,
                ],
            ),
        };
        let pre = build_fold(&snapshot(2, 3), Some(open), None);
        assert_eq!(
            value(&pre),
            golden_value(GOLDEN_PRE_EFFECT),
            "the pre_effect fold"
        );

        // pause: the sibling outcomes, the paused call excluded, the pending pause.
        let results = [
            ("{\"status\":\"ok\",\"result_count\":1}", ""),
            (
                "",
                "tool 'unknown_tool' is not in this agent's allowed scope",
            ),
        ];
        let completed = fold_completed(&calls, Some(2), results);
        let open = RoundInFlight {
            assistant_content: "Looking and deleting.".to_owned(),
            calls: calls.clone(),
            completed,
        };
        let fingerprint = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let pending = PendingPause {
            kind: PauseKind::Approval,
            pause_ref: "run-golden-w:tool_3_call-4".to_owned(),
            step_id: "tool_3_call-4".to_owned(),
            call: "delete_records".to_owned(),
            action_fingerprint: fingerprint.to_owned(),
        };
        let pause = build_fold(&snapshot(2, 3), Some(open), Some(pending));
        assert_eq!(value(&pause), golden_value(GOLDEN_PAUSE), "the pause fold");

        // Every fold the writer builds survives `fit`: scrubbed, gated, serialized.
        for fold in [round, pre, pause] {
            let bytes = fit(fold.clone()).expect("a golden fold fits");
            assert_eq!(LoopFoldV1::from_bytes(&bytes).expect("parses"), fold);
        }
    }

    #[test]
    fn a_fold_built_from_the_loop_locals_carries_every_counter() {
        // Distinct values for every field, so a field wired to the wrong local (or to
        // none) is visible.
        let retrievals = BTreeSet::from(["z-signature".to_owned(), "a-signature".to_owned()]);
        let messages = vec![
            chat("system", "the preamble"),
            pb::ChatMessage {
                compaction_summary: "kept-summary".to_owned(),
                role: "assistant".to_owned(),
                content: "an answer".to_owned(),
                name: "named".to_owned(),
            },
        ];
        let fold = build_fold(
            &Snapshot {
                run_id: "run-7",
                segment_no: 3,
                rounds_executed: 5,
                delegated_rounds: 2,
                step_seq: 11,
                children_started: 4,
                grounded: true,
                compaction_triggered: true,
                attempted_retrievals: &retrievals,
                messages: &messages,
                authority: authority(),
                prompt_digest: "the-digest",
                effect_ledger: true,
            },
            None,
            None,
        );
        assert_eq!(fold.v, FOLD_VERSION);
        assert_eq!(fold.run_id, "run-7");
        assert_eq!(fold.segment_no, 3);
        assert_eq!(fold.rounds_executed, 5);
        assert_eq!(fold.delegated_rounds, 2);
        assert_eq!(fold.step_seq, 11);
        assert_eq!(fold.children_started, 4);
        assert!(fold.grounded && fold.compaction_triggered);
        assert_eq!(
            fold.attempted_retrievals,
            vec!["a-signature".to_owned(), "z-signature".to_owned()],
            "sorted"
        );
        assert_eq!(fold.messages.len(), 2);
        assert_eq!(fold.messages[1].compaction_summary, "kept-summary");
        assert_eq!(fold.messages[1].name, "named");
        assert_eq!(fold.authority.digest(), authority().digest());
        assert_eq!(fold.prompt_digest, "the-digest");
        assert_eq!(fold.extensions[EXT_EFFECT_LEDGER], true);
        // Without the ledger the marker is absent, never `false`.
        let no_ledger = build_fold(
            &Snapshot {
                effect_ledger: false,
                ..Snapshot {
                    run_id: "run-7",
                    segment_no: 1,
                    rounds_executed: 1,
                    delegated_rounds: 0,
                    step_seq: 0,
                    children_started: 0,
                    grounded: false,
                    compaction_triggered: false,
                    attempted_retrievals: &retrievals,
                    messages: &messages,
                    authority: authority(),
                    prompt_digest: "",
                    effect_ledger: true,
                }
            },
            None,
            None,
        );
        assert!(!no_ledger.extensions.contains_key(EXT_EFFECT_LEDGER));
    }

    #[test]
    fn completed_outcomes_are_matched_to_their_calls_and_exclude_the_paused_one() {
        let tool_calls: Vec<pb::ToolCall> = ["a", "b", "c"]
            .iter()
            .map(|id| pb::ToolCall {
                id: (*id).to_owned(),
                name: format!("tool_{id}"),
                arguments_json: "{}".to_owned(),
            })
            .collect();
        let calls = fold_calls(&tool_calls, ["s1", "", "s3"]);
        assert_eq!(calls[1].step_id, "", "a rejected call has no step id");
        assert_eq!(calls[1].id, "b");

        // The first call is paused: the two outcomes belong to calls b and c.
        let completed = fold_completed(&calls, Some(0), [("out-b", ""), ("out-c", "")]);
        let ids: Vec<(&str, &str)> = completed
            .iter()
            .map(|c| (c.call_id.as_str(), c.step_id.as_str()))
            .collect();
        assert_eq!(ids, [("b", ""), ("c", "s3")]);
        assert_eq!(completed[0].output, "out-b");
        // The last call paused: the outcomes belong to a and b.
        let completed = fold_completed(&calls, Some(2), [("out-a", ""), ("", "boom")]);
        assert_eq!(completed.len(), 2);
        assert_eq!(completed[1].error, "boom");
        // No pause: every call has an outcome.
        let completed = fold_completed(&calls, None, [("1", ""), ("2", ""), ("3", "")]);
        assert_eq!(completed.len(), 3);
        // Fewer outcomes than calls never panics or invents one.
        assert_eq!(fold_completed(&calls, None, [("1", "")]).len(), 1);
    }

    /// The pre_effect fold resolves every call the loop refused before dispatch (it
    /// has an outcome already) and nothing else: a dispatchable call has none yet.
    #[test]
    fn rejected_calls_are_resolved_in_the_fold_and_dispatchable_calls_are_not() {
        let tool_calls: Vec<pb::ToolCall> = ["a", "b", "c", "d"]
            .iter()
            .map(|id| pb::ToolCall {
                id: (*id).to_owned(),
                name: format!("tool_{id}"),
                arguments_json: "{}".to_owned(),
            })
            .collect();
        let calls = fold_calls(&tool_calls, ["s1", "", "s3", ""]);
        let rejected = fold_rejected(
            &calls,
            [
                None,
                Some(("", "not in scope")),
                None,
                Some(("", "truncated")),
            ],
        );
        let resolved: Vec<(&str, &str, &str, &str)> = rejected
            .iter()
            .map(|c| {
                (
                    c.call_id.as_str(),
                    c.step_id.as_str(),
                    c.output.as_str(),
                    c.error.as_str(),
                )
            })
            .collect();
        assert_eq!(
            resolved,
            [("b", "", "", "not in scope"), ("d", "", "", "truncated")],
            "only the refused calls, in the model's order, with no step"
        );
        // A round with nothing refused resolves nothing; so does an empty round.
        assert!(fold_rejected(&calls, [None, None, None, None]).is_empty());
        assert!(fold_rejected(&[], []).is_empty());
        // More outcomes than calls never invents a call.
        assert_eq!(fold_rejected(&calls[..1], [None, Some(("", "x"))]).len(), 0);
    }

    /// A restored round's resolved sibling (one that ran before the loop stopped) is a call
    /// WITH a step whose outcome is already known: the fold keeps that step, so a reader sees
    /// a call that ran and not one that was refused before dispatch.
    #[test]
    fn a_resolved_call_that_ran_keeps_its_step_in_the_fold() {
        let tool_calls: Vec<pb::ToolCall> = ["a", "b"]
            .iter()
            .map(|id| pb::ToolCall {
                id: (*id).to_owned(),
                name: format!("tool_{id}"),
                arguments_json: "{}".to_owned(),
            })
            .collect();
        let calls = fold_calls(&tool_calls, ["tool_1_a", ""]);
        let resolved = fold_rejected(&calls, [Some(("done", "")), Some(("", "refused"))]);
        assert_eq!(
            resolved
                .iter()
                .map(|c| (c.call_id.as_str(), c.step_id.as_str()))
                .collect::<Vec<_>>(),
            [("a", "tool_1_a"), ("b", "")]
        );
    }

    fn big_tool_result(chars: usize) -> pb::ChatMessage {
        chat(
            "user",
            &format!(
                "Tool results for your previous request:\n{}",
                "x".repeat(chars)
            ),
        )
    }

    fn fold_with(messages: Vec<pb::ChatMessage>) -> LoopFoldV1 {
        let retrievals = BTreeSet::new();
        build_fold(
            &Snapshot {
                run_id: "run-1",
                segment_no: 1,
                rounds_executed: 1,
                delegated_rounds: 0,
                step_seq: 1,
                children_started: 0,
                grounded: false,
                compaction_triggered: false,
                attempted_retrievals: &retrievals,
                messages: &messages,
                authority: authority(),
                prompt_digest: "d",
                effect_ledger: true,
            },
            None,
            None,
        )
    }

    #[test]
    fn an_oversize_fold_is_cleared_once_then_refused() {
        // Stale tool results are what the one compaction pass can clear: six results
        // of 700 KB each are 4.2 MB, over the budget, but clearing all but the newest
        // three brings it far under.
        let mut messages = vec![chat("system", "preamble"), chat("user", "the goal")];
        messages.extend((0..6).map(|_| big_tool_result(700_000)));
        let fold = fold_with(messages);
        let bytes = fit(fold).expect("clearing the stale results makes it fit");
        assert!(bytes.len() <= SAVE_BUDGET_BYTES, "{} bytes", bytes.len());
        let parsed = LoopFoldV1::from_bytes(&bytes).expect("parses");
        let cleared = parsed
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .contains(crate::compaction_budget::CLEARED_TOOL_RESULT_NOTICE)
            })
            .count();
        assert_eq!(
            cleared, 3,
            "all but the newest three results were cleared, once"
        );

        // A goal that is itself over the budget cannot be cleared away: it is refused,
        // never truncated.
        let huge_goal = fold_with(vec![
            chat("system", "preamble"),
            chat("user", &"g".repeat(SAVE_BUDGET_BYTES + 1024)),
        ]);
        assert_eq!(
            fit(huge_goal),
            Err(SaveError::ContentRefused { reason: "oversize" })
        );
    }

    /// A message that is credential-shaped after scrubbing is never replaced by a stub
    /// inside a fold that stays resumable: a resume would continue on a damaged
    /// conversation. The fold is refused as a credential (the writer saves a tombstone).
    #[test]
    fn a_credential_shaped_message_refuses_the_fold_instead_of_stubbing_it() {
        let credential = Err(SaveError::ContentRefused {
            reason: "credential",
        });
        for (label, fold) in [
            (
                "the goal",
                fold_with(vec![
                    chat("system", "preamble"),
                    chat("user", "send an Authorization: header"),
                ]),
            ),
            (
                "a system message",
                fold_with(vec![
                    chat("system", "preamble"),
                    chat("system", "skills: set the x-api-key header"),
                    chat("user", "the goal"),
                ]),
            ),
            (
                "a tool result",
                fold_with(vec![
                    chat("system", "preamble"),
                    chat("user", "the goal"),
                    chat(
                        "user",
                        "Tool results for your previous request: -----BEGIN CERTIFICATE-----",
                    ),
                ]),
            ),
        ] {
            assert_eq!(fit(fold), credential, "{label}");
        }
        // The open round's text and outcomes are conversation too.
        let mut fold = fold_with(vec![chat("system", "p"), chat("user", "g")]);
        fold.round_in_flight = Some(RoundInFlight {
            assistant_content: "the Authorization: header is needed".to_owned(),
            calls: Vec::new(),
            completed: Vec::new(),
        });
        assert_eq!(fit(fold), credential, "the open round's assistant text");
        let mut fold = fold_with(vec![chat("system", "p"), chat("user", "g")]);
        fold.round_in_flight = Some(RoundInFlight {
            assistant_content: String::new(),
            calls: Vec::new(),
            completed: vec![FoldCompleted {
                step_id: "s1".to_owned(),
                output: "x-api-key: abc".to_owned(),
                error: String::new(),
                call_id: "c1".to_owned(),
            }],
        });
        assert_eq!(fit(fold), credential, "a completed outcome");
    }

    /// A secret in a call's arguments is withheld by design: the fold is still saved and
    /// still resumable, the call carries empty arguments (never redispatched), and the
    /// secret is nowhere in the bytes.
    #[test]
    fn a_secret_in_call_arguments_is_withheld_and_the_fold_stays_resumable() {
        let secret = format!("sk-{}", "foldargsecretkey1234567890abcd");
        let mut fold = fold_with(vec![chat("system", "p"), chat("user", "g")]);
        fold.round_in_flight = Some(RoundInFlight {
            assistant_content: "Deleting.".to_owned(),
            calls: vec![FoldCall {
                id: "c1".to_owned(),
                name: "delete_records".to_owned(),
                arguments_json: format!(r#"{{"table":"stale","note":"{secret}"}}"#),
                step_id: "tool_1_c1".to_owned(),
            }],
            completed: Vec::new(),
        });
        let bytes = fit(fold).expect("withheld arguments do not refuse the fold");
        assert!(!String::from_utf8_lossy(&bytes).contains(&secret));
        let parsed = LoopFoldV1::from_bytes(&bytes).expect("parses");
        assert!(parsed.is_resumable());
        let call = &parsed.round_in_flight.as_ref().expect("open round").calls[0];
        assert_eq!(call.arguments_json, "");
        assert_eq!(call.step_id, "tool_1_c1", "the call can still be matched");
    }

    #[test]
    fn the_save_budget_is_under_the_transport_limit() {
        // tonic's default decode limit and session-core's plaintext cap are both
        // 4 MiB; the request carries the fold plus a few short fields.
        const TONIC_DEFAULT_DECODE_LIMIT: usize = 4 * 1024 * 1024;
        assert!(SAVE_BUDGET_BYTES + 64 * 1024 < TONIC_DEFAULT_DECODE_LIMIT);
        assert!(SAVE_BUDGET_BYTES < lf::MAX_FOLD_BYTES);
    }

    #[test]
    fn seq_tracker_starts_at_zero_advances_and_probes_once_after_an_ambiguous_failure() {
        let mut tracker = SeqTracker::default();
        assert_eq!(tracker.expected(), 0, "the first save expects 0");
        tracker.on_saved(1);
        assert_eq!(tracker.expected(), 1);
        tracker.on_saved(2);
        assert_eq!(tracker.expected(), 2);

        // A certain failure never advances and never arms a probe.
        tracker.on_failed(&SaveError::Unavailable { ambiguous: false });
        tracker.on_failed(&SaveError::ContentRefused { reason: "oversize" });
        assert_eq!(tracker.expected(), 2);
        assert_eq!(tracker.probe(), None, "nothing may have committed");

        // An ambiguous failure arms exactly one probe at expected + 1.
        tracker.on_failed(&SaveError::Unavailable { ambiguous: true });
        assert_eq!(
            tracker.expected(),
            2,
            "the expectation itself does not move"
        );
        assert_eq!(tracker.probe(), Some(3));
        assert_eq!(
            tracker.probe(),
            None,
            "the probe is spent: a second stale is a second writer"
        );

        // A success clears any pending uncertainty.
        tracker.on_failed(&SaveError::Unavailable { ambiguous: true });
        tracker.on_saved(4);
        assert_eq!(tracker.expected(), 4);
        assert_eq!(tracker.probe(), None);
    }

    fn status_with_trailer(
        code: tonic::Code,
        refusal: mp_contracts::zdr_refusal::Refusal,
    ) -> tonic::Status {
        refusal.grpc_status(code)
    }

    #[test]
    fn session_core_refusals_are_classified() {
        use tonic::{Code, Status};
        assert_eq!(
            classify_status(&Status::aborted("loop_checkpoint_stale")),
            SaveError::Stale
        );
        // Closed, and a run that has ended: nothing more may be written, no retry.
        assert_eq!(
            classify_status(&Status::failed_precondition("loop_checkpoint_closed")),
            SaveError::Closed
        );
        assert_eq!(
            classify_status(&Status::failed_precondition("loop_checkpoint_run_ended")),
            SaveError::Closed,
            "a run that ended is classified like a closed fold: the writer fails, no retry"
        );
        assert_eq!(
            classify_status(&Status::failed_precondition("loop_checkpoint_disabled")),
            SaveError::StoreOff
        );
        // A ZDR-catalogue refusal, by its trailer, whatever the code.
        for code in [Code::FailedPrecondition, Code::PermissionDenied] {
            assert_eq!(
                classify_status(&status_with_trailer(
                    code,
                    mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
                )),
                SaveError::NotPersistent,
                "{code:?}"
            );
        }
        for code in [Code::InvalidArgument, Code::OutOfRange] {
            assert!(
                matches!(
                    classify_status(&Status::new(code, "x")),
                    SaveError::ContentRefused { .. }
                ),
                "{code:?}"
            );
        }
        // Throttling is about the load, not the content: transient, and it did not commit.
        assert_eq!(
            classify_status(&Status::resource_exhausted("slow down")),
            SaveError::Unavailable { ambiguous: false },
            "ResourceExhausted must not tombstone a fold that would save a moment later"
        );
        // A deterministic refusal of this run or caller: the same answer next time, so
        // the writer fails at once. One closed token per cause.
        for (status, by, token) in [
            (
                Status::not_found("run not found"),
                RefusedBy::NotFound,
                "not_found",
            ),
            (Status::permission_denied("no"), RefusedBy::Denied, "denied"),
            (
                Status::unauthenticated("loop checkpoint did not authenticate"),
                RefusedBy::Denied,
                "denied",
            ),
            (
                Status::unimplemented("save_loop_checkpoint"),
                RefusedBy::Unimplemented,
                "unimplemented",
            ),
            (
                Status::failed_precondition("loop checkpoint key unavailable"),
                RefusedBy::KeyUnavailable,
                "key_unavailable",
            ),
        ] {
            let error = classify_status(&status);
            assert_eq!(error, SaveError::Refused(by), "{status:?}");
            assert_eq!(error.as_str(), "refused");
            assert_eq!(error.cause(), token);
        }
        for code in [
            Code::Unavailable,
            Code::DeadlineExceeded,
            Code::Cancelled,
            Code::Unknown,
            Code::Internal,
            Code::DataLoss,
        ] {
            assert_eq!(
                classify_status(&Status::new(code, "x")),
                SaveError::Unavailable { ambiguous: true },
                "{code:?}"
            );
        }
        // The rest did not commit: certain, and transient. (A FailedPrecondition with
        // another message is not the key refusal.)
        for status in [
            Status::aborted("some other conflict"),
            Status::failed_precondition("some other precondition"),
            Status::already_exists("x"),
        ] {
            assert_eq!(
                classify_status(&status),
                SaveError::Unavailable { ambiguous: false },
                "{status:?}"
            );
        }
        // The stale token is read only from an Aborted status.
        assert_eq!(
            classify_status(&Status::unavailable("loop_checkpoint_stale")),
            SaveError::Unavailable { ambiguous: true }
        );
    }

    #[test]
    fn a_tombstone_is_a_valid_fold_that_check_fold_accepts() {
        let header = Header::of(&fold_with(vec![chat("system", "p"), chat("user", "g")]));
        let tombstone = header.tombstone("oversize");
        let bytes = tombstone.to_bytes().expect("serializes");
        // The same checks session-core's `check_fold` makes: v1, JSON, this run.
        let parsed: LoopFoldV1 = serde_json::from_slice(&bytes).expect("a v1 JSON object");
        assert_eq!(parsed.v, FOLD_VERSION);
        assert_eq!(parsed.run_id, "run-1");
        assert!(!parsed.is_resumable());
        assert!(bytes.len() < 1024, "small: {} bytes", bytes.len());
        assert_eq!(
            parsed.authority.digest(),
            authority().digest(),
            "an authority digest exists to save under"
        );
    }

    /// What a content-refused PAUSE fold is replaced by is built from the refused fold's
    /// header alone: counters and authority, the refusal's closed word, and nothing of the
    /// conversation, the open round or the pause the refused fold carried.
    #[test]
    fn a_pause_tombstone_carries_the_refusal_reason_and_none_of_the_refused_fold() {
        let secret = format!("sk-{}", "pausetombstonesecret1234567890");
        let mut refused = fold_with(vec![
            chat("system", "p"),
            chat("user", &format!("the key is {secret}")),
        ]);
        refused.pending = Some(PendingPause {
            kind: PauseKind::Approval,
            pause_ref: "run-1:tool_1_c1".to_owned(),
            step_id: "tool_1_c1".to_owned(),
            call: "delete_records".to_owned(),
            action_fingerprint: "fp".to_owned(),
        });
        refused.round_in_flight = Some(RoundInFlight {
            assistant_content: format!("using {secret}"),
            calls: Vec::new(),
            completed: Vec::new(),
        });
        let header = Header::of(&refused);
        for reason in ["credential", "oversize", "rejected", "malformed"] {
            let (bytes, digest) = header.tombstone_bytes(reason).expect("a tombstone serializes");
            let parsed = LoopFoldV1::from_bytes(&bytes).expect("a valid fold");
            assert!(!parsed.is_resumable(), "{reason}");
            assert_eq!(
                parsed.extensions[lf::EXT_NOT_RESUMABLE], reason,
                "the closed reason the refusal carried"
            );
            assert!(
                parsed.messages.is_empty()
                    && parsed.round_in_flight.is_none()
                    && parsed.pending.is_none(),
                "{reason}: no conversation, no open round, no pause"
            );
            assert_eq!(digest, refused.authority.digest(), "{reason}");
            assert!(
                !String::from_utf8_lossy(&bytes).contains(&secret),
                "{reason}: the refused content never reaches the tombstone"
            );
        }
        // The counters and the run are the refused fold's own.
        let (bytes, _) = header.tombstone_bytes("credential").expect("serializes");
        let parsed = LoopFoldV1::from_bytes(&bytes).expect("parses");
        assert_eq!(
            (parsed.run_id.as_str(), parsed.segment_no, parsed.rounds_executed, parsed.step_seq),
            (refused.run_id.as_str(), refused.segment_no, refused.rounds_executed, refused.step_seq)
        );
    }

    #[test]
    fn the_held_back_text_names_only_the_tool() {
        let text = held_back_text("delete_records");
        assert!(text.contains("'delete_records'"));
        assert!(text.contains("Try again."));
        // Nothing of a failure can reach it: it takes only the tool name.
        assert!(!text.contains("loop_checkpoint"));
    }

    #[test]
    fn the_text_for_a_failed_writer_names_only_the_tool_and_never_invites_a_retry() {
        let text = held_for_good_text("delete_records");
        assert!(text.contains("'delete_records'"));
        assert!(text.contains("was not run") && text.contains("could not be started safely"));
        let lower = text.to_lowercase();
        assert!(
            !lower.contains("try again") && !lower.contains("retry"),
            "a failed writer never saves again: {text}"
        );
        assert!(!text.contains("loop_checkpoint"));
        assert_ne!(text, held_back_text("delete_records"));
    }

    #[tokio::test]
    async fn a_refusal_that_will_not_change_fails_the_writer_at_once() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        for by in [
            RefusedBy::NotFound,
            RefusedBy::Denied,
            RefusedBy::Unimplemented,
            RefusedBy::KeyUnavailable,
        ] {
            let mut writer = FoldWriter::new(&channel, Some("b"), "run-1", all_good());
            assert!(writer.is_active() && !writer.is_failed());
            writer.apply(SaveError::Refused(by));
            assert!(
                writer.is_failed() && writer.engaged() && !writer.is_active(),
                "{by:?}"
            );
            // A failed writer still engages the effect gate, so effects are held.
            assert_eq!(writer.off_reason(), None);
        }
        // The retryable and the switch-off classes are unchanged.
        let mut writer = FoldWriter::new(&channel, Some("b"), "run-1", all_good());
        writer.apply(SaveError::Unavailable { ambiguous: false });
        assert!(
            writer.is_active(),
            "an unreachable store is retried next round"
        );
        writer.apply(SaveError::StoreOff);
        assert_eq!(writer.off_reason(), Some(OffReason::StoreDisabled));
    }

    #[test]
    fn a_seeded_tracker_expects_the_claimed_seq_and_has_nothing_to_probe() {
        let mut tracker = SeqTracker::seeded(7);
        assert_eq!(tracker.expected(), 7);
        assert_eq!(
            tracker.probe(),
            None,
            "no earlier save of this writer may have committed"
        );
        tracker.on_saved(8);
        assert_eq!(tracker.expected(), 8);
        // A seeded tracker probes like any other after an ambiguous failure.
        tracker.on_failed(&SaveError::Unavailable { ambiguous: true });
        assert_eq!(tracker.probe(), Some(9));
        assert_eq!(SeqTracker::seeded(0), SeqTracker::default());
    }

    #[tokio::test]
    async fn a_seeded_writer_expects_the_claimed_seq_and_writes_under_its_segment() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let fresh = FoldWriter::new(&channel, Some("b"), "run-1", all_good());
        assert_eq!((fresh.expected_seq(), fresh.segment()), (0, 1));
        let seeded = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 12, 3);
        assert_eq!((seeded.expected_seq(), seeded.segment()), (12, 3));
        assert!(
            seeded.is_active(),
            "an open gate and a bearer: the writer writes"
        );
        let no_bearer = FoldWriter::seeded(&channel, None, "run-1", all_good(), 12, 3);
        assert_eq!(no_bearer.off_reason(), Some(OffReason::NoBearer));
        assert_eq!(no_bearer.expected_seq(), 12);
    }

    /// One gate state per refusing reason, the control first.
    fn refusing_gates() -> Vec<(WriterGate, OffReason)> {
        vec![
            (
                WriterGate {
                    switch: false,
                    ..all_good()
                },
                OffReason::SwitchOff,
            ),
            (
                WriterGate {
                    ledger: false,
                    ..all_good()
                },
                OffReason::LedgerOff,
            ),
            (
                WriterGate {
                    zdr: true,
                    ..all_good()
                },
                OffReason::Zdr,
            ),
            (
                WriterGate {
                    waived: true,
                    ..all_good()
                },
                OffReason::Waived,
            ),
            (
                WriterGate {
                    space: true,
                    ..all_good()
                },
                OffReason::Space,
            ),
            (
                WriterGate {
                    depth: 1,
                    ..all_good()
                },
                OffReason::Delegated,
            ),
        ]
    }

    /// `resumed` is the stage-1 segment flag and only [`FoldWriter::new`] honours it: an
    /// in-place resume is never a stage-1 segment, so [`FoldWriter::seeded`] forces it false
    /// itself. The decision a caller makes before it claims (`resume::in_place_writer_off`)
    /// is therefore the writer's own whatever the caller put in `resumed`.
    #[tokio::test]
    async fn only_a_fresh_writer_honours_the_stage_one_segment_flag() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let resumed = WriterGate {
            resumed: true,
            ..all_good()
        };
        let fresh = FoldWriter::new(&channel, Some("b"), "run-1", resumed);
        assert_eq!(fresh.off_reason(), Some(OffReason::ResumedSegment));
        let seeded = FoldWriter::seeded(&channel, Some("b"), "run-1", resumed, 12, 3);
        assert!(
            seeded.is_active(),
            "an in-place resume is not a stage-1 segment, whatever the gate says"
        );
        assert_eq!((seeded.expected_seq(), seeded.segment()), (12, 3));
        // Every other refusal still applies to it, with `resumed` set or not.
        for (gate, reason) in refusing_gates() {
            let writer = FoldWriter::seeded(
                &channel,
                Some("b"),
                "run-1",
                WriterGate {
                    resumed: true,
                    ..gate
                },
                12,
                3,
            );
            assert_eq!(writer.off_reason(), Some(reason), "{gate:?}");
        }
    }

    /// A listener nothing serves, standing in for session-core: a writer that makes an
    /// RPC opens a connection to it (the TCP handshake completes in the backlog even
    /// though nobody accepts), a writer that makes none leaves it untouched.
    struct Probe {
        listener: std::net::TcpListener,
    }

    impl Probe {
        fn bind() -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            listener.set_nonblocking(true).expect("non-blocking");
            Self { listener }
        }

        fn channel(&self) -> Channel {
            let addr = self.listener.local_addr().expect("addr");
            tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
                .expect("endpoint")
                .connect_lazy()
        }

        /// How many connections were opened to it so far.
        fn connections(&self) -> usize {
            let mut count = 0;
            while self.listener.accept().is_ok() {
                count += 1;
            }
            count
        }
    }

    /// Every save entry point of a writer, driven once.
    async fn drive_every_save(writer: &mut FoldWriter<'_>) -> PreEffect {
        let fold = || fold_with(vec![chat("system", "p"), chat("user", "g")]);
        writer.save_round(fold()).await;
        let pre_effect = writer.save_pre_effect(fold()).await;
        if let Some(mut pause) = writer.pause_fold(fold()) {
            pause.save("tool_1_c1", "delete_records", "fp").await;
            pause.close_after_failed_approval().await;
        }
        pre_effect
    }

    /// The seeded constructor cannot be the way round the gate: a caller that builds a
    /// resumed writer without gating the run first must still get no folds for a ZDR, a
    /// waived, a Space or a delegated run, nor with the switch or the ledger off.
    #[tokio::test]
    async fn a_seeded_writer_under_every_refusing_gate_is_off_and_makes_no_call() {
        let probe = Probe::bind();
        let channel = probe.channel();
        for (gate, reason) in refusing_gates() {
            let mut writer = FoldWriter::seeded(&channel, Some("b"), "run-1", gate, 12, 3);
            assert_eq!(writer.off_reason(), Some(reason), "{gate:?}");
            assert!(
                !writer.is_active() && !writer.engaged() && !writer.is_failed(),
                "{gate:?}"
            );
            // The seed is still what the resume handed over, for the log and the tests.
            assert_eq!((writer.expected_seq(), writer.segment()), (12, 3));
            // An off writer lets the effect through, as with the switch off.
            assert_eq!(
                drive_every_save(&mut writer).await,
                PreEffect::Degraded,
                "{gate:?}"
            );
            assert!(writer.pause_fold(fold_with(Vec::new())).is_none(), "{gate:?}");
            assert_eq!(writer.expected_seq(), 12, "{gate:?}: nothing was saved");
        }
        assert_eq!(probe.connections(), 0, "no gate state made an RPC");
    }

    /// The same, over the posture matrix rows the resolver serves: a persistent row
    /// seeds an active writer, a ZDR or a waived row an off one, for the right reason.
    #[tokio::test]
    async fn a_seeded_writer_follows_the_posture_matrix_row_by_row() {
        use mp_contracts::retention::{
            resolve_turn, ClaimsView, Resolution, RouteView, WaiverView,
        };
        const MATRIX: &str = include_str!("../../tests/fixtures/zdr-posture-matrix.json");
        let matrix: serde_json::Value = serde_json::from_str(MATRIX).expect("the matrix is JSON");
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let (mut on, mut off) = (0_usize, 0_usize);
        for row in matrix["enforcement_rows"].as_array().expect("rows") {
            let id = row["id"].as_str().unwrap_or("?");
            let claims: Vec<ClaimsView<'_>> = row["inputs"]["bearers"]
                .as_array()
                .expect("bearers")
                .iter()
                .map(|bearer| ClaimsView {
                    zdr: bearer["zdr"].as_bool().expect("zdr"),
                    org_zdr: bearer["org_zdr"].as_bool(),
                    waiver: (!bearer["waiver"].is_null()).then(|| WaiverView {
                        version: u32::try_from(bearer["waiver"]["version"].as_u64().expect("v"))
                            .expect("fits"),
                        provider: bearer["waiver"]["provider"].as_str().expect("provider"),
                        connection: bearer["waiver"]["connection"].as_str().expect("connection"),
                    }),
                })
                .collect();
            let route = RouteView {
                provider: row["inputs"]["route"]["provider"]
                    .as_str()
                    .expect("provider"),
                connection: row["inputs"]["route"]["connection"].as_str(),
                explicit_zdr: row["inputs"]["route"]["explicit_zdr"]
                    .as_bool()
                    .expect("zdr"),
            };
            let (zdr, waived, expected) = match resolve_turn(&claims, &route) {
                Resolution::Refuse(_) => continue,
                Resolution::Persistent { .. } => (false, false, None),
                Resolution::Zdr { .. } => (true, false, Some(OffReason::Zdr)),
                Resolution::Waived { .. } => (false, true, Some(OffReason::Waived)),
            };
            let gate = WriterGate {
                zdr,
                waived,
                ..all_good()
            };
            let writer = FoldWriter::seeded(&channel, Some("b"), "run-1", gate, 5, 2);
            assert_eq!(writer.off_reason(), expected, "{id}");
            assert_eq!(writer.is_active(), expected.is_none(), "{id}");
            if expected.is_none() {
                on += 1;
            } else {
                off += 1;
            }
        }
        assert_eq!((on, off), (3, 6), "every served row was checked");
    }

    /// The control for the no-call proofs: the same drive against an ACTIVE writer does
    /// reach the listener, so a zero is the gate's doing and not an idle harness.
    #[tokio::test]
    async fn a_seeded_writer_with_an_open_gate_does_reach_the_store() {
        let probe = Probe::bind();
        let channel = probe.channel();
        let mut writer = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 12, 3);
        assert!(writer.is_active());
        writer
            .save_round(fold_with(vec![chat("system", "p"), chat("user", "g")]))
            .await;
        assert!(probe.connections() >= 1, "an open gate makes the RPC");
    }

    // ---- the fence of an in-place resume (WS03 S7b) ----

    /// What the injected fence RPC saw, in order.
    type FenceCalls = std::sync::Arc<std::sync::Mutex<Vec<(u64, SaveKind, String, usize)>>>;

    /// Run `fence_with` against an RPC that answers each call from `answers` in order, and
    /// report what it was asked.
    async fn fence_through(
        writer: &mut FoldWriter<'_>,
        fold: LoopFoldV1,
        answers: Vec<Result<u64, SaveError>>,
    ) -> (Result<u64, SaveError>, Vec<(u64, SaveKind, String, usize)>) {
        let calls: FenceCalls = FenceCalls::default();
        let recorder = std::sync::Arc::clone(&calls);
        let mut answers = answers.into_iter();
        let result = writer
            .fence_with(fold, |save| {
                recorder.lock().unwrap().push((
                    save.expected_seq,
                    save.kind,
                    save.pause_ref.clone(),
                    save.bytes.len(),
                ));
                let answer = answers.next().expect("the fence made a call nobody answered");
                async move { answer }
            })
            .await;
        let calls = calls.lock().unwrap().clone();
        (result, calls)
    }

    fn fenceable_fold() -> LoopFoldV1 {
        fold_with(vec![chat("system", "p"), chat("user", "g")])
    }

    /// The fence is the whole defence against a second claimant: ONE save of kind
    /// `pre_effect` at the seq the claim returned, the seq compare-and-set deciding who
    /// proceeds. A stale answer means another claimant won, and it is final: the probe a
    /// round save makes after an ambiguous failure would overwrite that claimant.
    #[tokio::test]
    async fn the_fence_is_one_save_at_the_claimed_seq_with_no_probe() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let mut writer = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 12, 3);

        let (result, calls) = fence_through(&mut writer, fenceable_fold(), vec![Ok(13)]).await;
        assert_eq!(result, Ok(13));
        assert_eq!(calls.len(), 1, "exactly one RPC");
        let (expected, kind, pause_ref, bytes) = &calls[0];
        assert_eq!(*expected, 12, "at the claimed seq");
        assert_eq!(*kind, SaveKind::PreEffect);
        assert_eq!(pause_ref, "", "a fence is not a pause");
        assert!(*bytes > 0);
        assert_eq!(writer.expected_seq(), 13, "the writer chains from the fence");
        assert!(writer.is_active());

        // The probe case: an ambiguous failure makes a normal save retry a stale answer once
        // with `expected + 1`. The fence never does: one call, and the stale answer is final.
        writer.tracker.on_failed(&SaveError::Unavailable { ambiguous: true });
        let (stale, calls) = fence_through(&mut writer, fenceable_fold(), vec![Err(SaveError::Stale)]).await;
        assert_eq!(stale, Err(SaveError::Stale));
        assert_eq!(
            calls.iter().map(|call| call.0).collect::<Vec<_>>(),
            vec![13],
            "one call, at the expected seq, and no retry at 14"
        );
        assert!(writer.is_failed(), "another claimant won: this writer stops for good");
    }

    #[tokio::test]
    async fn a_fence_on_a_writer_that_is_off_or_failed_makes_no_call_and_fails() {
        let probe = Probe::bind();
        let channel = probe.channel();
        // Off: the gate says no, or there is no credential.
        for (writer_gate, bearer) in [
            (WriterGate { switch: false, ..all_good() }, Some("b")),
            (WriterGate { ledger: false, ..all_good() }, Some("b")),
            (WriterGate { zdr: true, ..all_good() }, Some("b")),
            (WriterGate { waived: true, ..all_good() }, Some("b")),
            (WriterGate { space: true, ..all_good() }, Some("b")),
            (all_good(), None),
        ] {
            let mut writer = FoldWriter::seeded(&channel, bearer, "run-1", writer_gate, 12, 3);
            assert!(!writer.is_active());
            let (result, calls) = fence_through(&mut writer, fenceable_fold(), Vec::new()).await;
            assert_eq!(result, Err(SaveError::Unavailable { ambiguous: false }), "{writer_gate:?}");
            assert!(calls.is_empty(), "{writer_gate:?}: no RPC from a writer that is off");
            // The real entry point makes no connection either.
            let result = writer.save_fence(fenceable_fold()).await;
            assert_eq!(result, Err(SaveError::Unavailable { ambiguous: false }));
            assert_eq!(writer.expected_seq(), 12, "nothing moved");
        }
        // Failed: a writer that lost its claim, or whose store refused it for good.
        for failure in [
            SaveError::Stale,
            SaveError::Closed,
            SaveError::Refused(RefusedBy::Denied),
        ] {
            let mut writer = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 12, 3);
            writer.apply(failure);
            assert!(writer.is_failed());
            let (result, calls) = fence_through(&mut writer, fenceable_fold(), Vec::new()).await;
            assert_eq!(result, Err(SaveError::Unavailable { ambiguous: false }), "{failure:?}");
            assert!(calls.is_empty(), "{failure:?}");
            assert!(writer.save_fence(fenceable_fold()).await.is_err());
        }
        assert_eq!(probe.connections(), 0, "no writer that is not active reached the store");
    }

    /// A pre_effect save degrades a refused fold to a tombstone and dispatches anyway. A
    /// fence must not: without a durable fence at the claimed seq nothing stops a second
    /// claimant, so a refused fence is an error and the resume dispatches nothing.
    #[tokio::test]
    async fn a_content_refused_fence_is_an_error_not_a_tombstone() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        // Refused here, by the scrub and the credential gate: no RPC at all, and no tombstone.
        let mut writer = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 12, 3);
        let credential = fold_with(vec![chat("system", "p"), chat("user", "send an Authorization: header")]);
        let (result, calls) = fence_through(&mut writer, credential, Vec::new()).await;
        assert_eq!(result, Err(SaveError::ContentRefused { reason: "credential" }));
        assert!(calls.is_empty(), "a refused fold is not saved, and no tombstone replaces it");
        assert_eq!(writer.expected_seq(), 12);
        assert!(writer.is_active(), "a refused fence leaves the writer as it was");

        // Refused there, by session-core as invalid: ONE call, and no tombstone call after it.
        let (rejected, calls) = fence_through(
            &mut writer,
            fenceable_fold(),
            vec![Err(SaveError::ContentRefused { reason: "rejected" })],
        )
        .await;
        assert_eq!(rejected, Err(SaveError::ContentRefused { reason: "rejected" }));
        assert_eq!(calls.len(), 1, "no tombstone follow-up");
        assert_eq!(writer.expected_seq(), 12);
        assert!(writer.is_active());
    }

    // ---- the park (WS03 S8-A, S8-5) ----

    /// What the injected park RPC saw, in order: (expected seq, `pause_ref`, reason, due, bytes).
    type ParkCalls =
        std::sync::Arc<std::sync::Mutex<Vec<(u64, String, ParkReason, DateTime<Utc>, usize)>>>;

    fn due() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-19T12:00:00Z")
            .expect("a timestamp")
            .with_timezone(&Utc)
    }

    /// A fold that names its park, as `park::try_park` builds it.
    fn parkable_fold() -> LoopFoldV1 {
        let mut fold = fenceable_fold();
        fold.pending = Some(PendingPause {
            kind: PauseKind::Park,
            pause_ref: "park:run-1:1".to_owned(),
            step_id: String::new(),
            call: String::new(),
            action_fingerprint: String::new(),
        });
        fold
    }

    /// Run `park_with` against an RPC that answers each call from `answers` in order, and
    /// report what it was asked.
    async fn park_through(
        writer: &mut FoldWriter<'_>,
        fold: LoopFoldV1,
        answers: Vec<Result<(u64, bool), SaveError>>,
    ) -> (Result<u64, ParkSaveError>, Vec<(u64, String, ParkReason, DateTime<Utc>, usize)>) {
        let calls: ParkCalls = ParkCalls::default();
        let recorder = std::sync::Arc::clone(&calls);
        let mut answers = answers.into_iter();
        let result = writer
            .park_with(fold, ParkReason::ProviderExhausted, due(), |save| {
                recorder.lock().unwrap().push((
                    save.expected_seq,
                    save.pause_ref.clone(),
                    save.reason,
                    save.park_until,
                    save.bytes.len(),
                ));
                let answer = answers.next().expect("the park made a call nobody answered");
                async move { answer }
            })
            .await;
        let calls = calls.lock().unwrap().clone();
        (result, calls)
    }

    #[test]
    fn a_park_is_its_own_save_kind_with_the_park_word_and_the_gated_timeout() {
        assert_eq!(SaveKind::Park.as_str(), "park");
        assert_eq!(SaveKind::Park.timeout(), GATED_SAVE_TIMEOUT);
    }

    /// A park is ONE save at the expected seq, no probe: a probe after an ambiguous failure
    /// would write at `expected + 1`, and a park that may have committed is closed by its
    /// caller instead. The seq moves only on an accepted park.
    #[tokio::test]
    async fn save_park_is_one_rpc_at_the_expected_seq_with_no_probe() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let mut writer = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 4, 2);

        let (result, calls) = park_through(&mut writer, parkable_fold(), vec![Ok((5, true))]).await;
        assert_eq!(result, Ok(5));
        assert_eq!(calls.len(), 1, "exactly one RPC");
        let (expected, pause_ref, reason, until, bytes) = &calls[0];
        assert_eq!(*expected, 4, "at the expected seq");
        assert_eq!(pause_ref, "park:run-1:1", "the fold's own park reference");
        assert_eq!(*reason, ParkReason::ProviderExhausted);
        assert_eq!(*until, due());
        assert!(*bytes > 0);
        assert_eq!(writer.expected_seq(), 5, "an accepted park moves the seq");
        assert!(writer.is_active());

        // The probe case: an ambiguous failure makes a round save retry a stale answer once
        // with `expected + 1`. A park never does: one call, and the stale answer is final.
        writer.tracker.on_failed(&SaveError::Unavailable { ambiguous: true });
        let (stale, calls) =
            park_through(&mut writer, parkable_fold(), vec![Err(SaveError::Stale)]).await;
        assert_eq!(stale, Err(ParkSaveError::Save(SaveError::Stale)));
        assert_eq!(
            calls.iter().map(|call| call.0).collect::<Vec<_>>(),
            vec![5],
            "one call, at the expected seq, and no retry at 6"
        );
        assert!(writer.is_failed(), "a stale answer stops the writer, as for any save");

        // An ambiguous failure leaves the writer active and the seq where it was: the
        // caller closes `expected + 1`.
        let mut writer = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 4, 2);
        let (lost, calls) = park_through(
            &mut writer,
            parkable_fold(),
            vec![Err(SaveError::Unavailable { ambiguous: true })],
        )
        .await;
        assert_eq!(
            lost,
            Err(ParkSaveError::Save(SaveError::Unavailable { ambiguous: true }))
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(writer.expected_seq(), 4);
        assert!(writer.is_active());
    }

    /// A store that stored the fold but did not say it took a park (one that predates the
    /// echo, or has park recovery off) has NOT taken it: the caller is told which row to
    /// close, and the writer is left as it was.
    #[tokio::test]
    async fn a_park_save_without_park_accepted_is_not_accepted() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let mut writer = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 4, 2);
        let (result, calls) =
            park_through(&mut writer, parkable_fold(), vec![Ok((5, false))]).await;
        assert_eq!(result, Err(ParkSaveError::NotAccepted { seq: 5 }));
        assert_eq!(calls.len(), 1);
        assert_eq!(writer.expected_seq(), 4, "the seq moves only on an accepted park");
        assert!(writer.is_active());
    }

    /// A round or pre-effect save degrades a refused fold to a tombstone. A park must not: a
    /// tombstone saved as the park is a park nothing can resume.
    #[tokio::test]
    async fn a_content_refused_park_is_an_error_never_a_tombstone() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        let mut writer = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 4, 2);
        // Refused here, by the scrub and the credential gate: no RPC at all.
        let mut credential = parkable_fold();
        credential.messages.push(FoldMessage {
            role: "user".to_owned(),
            content: "send an Authorization: header".to_owned(),
            name: String::new(),
            compaction_summary: String::new(),
        });
        let (result, calls) = park_through(&mut writer, credential, Vec::new()).await;
        assert_eq!(
            result,
            Err(ParkSaveError::Save(SaveError::ContentRefused {
                reason: "credential"
            }))
        );
        assert!(calls.is_empty(), "a refused fold is not saved, and no tombstone replaces it");
        assert_eq!(writer.expected_seq(), 4);
        assert!(writer.is_active(), "a refused park leaves the writer as it was");

        // Refused there, by session-core as invalid: ONE call and no tombstone call after it.
        let (rejected, calls) = park_through(
            &mut writer,
            parkable_fold(),
            vec![Err(SaveError::ContentRefused { reason: "rejected" })],
        )
        .await;
        assert_eq!(
            rejected,
            Err(ParkSaveError::Save(SaveError::ContentRefused {
                reason: "rejected"
            }))
        );
        assert_eq!(calls.len(), 1, "no tombstone follow-up");
        assert!(writer.is_active());

        // A fold that does not name its park is refused before any call.
        for fold in [fenceable_fold(), {
            let mut approval = parkable_fold();
            approval.pending.as_mut().expect("pending").kind = PauseKind::Approval;
            approval
        }, {
            let mut blank = parkable_fold();
            blank.pending.as_mut().expect("pending").pause_ref.clear();
            blank
        }] {
            let (result, calls) = park_through(&mut writer, fold, Vec::new()).await;
            assert_eq!(
                result,
                Err(ParkSaveError::Save(SaveError::ContentRefused {
                    reason: "malformed"
                }))
            );
            assert!(calls.is_empty());
        }
    }

    /// A writer that is off or failed makes no park call, and says so with the closed
    /// "no RPC was made" answer.
    #[tokio::test]
    async fn a_park_on_a_writer_that_is_off_or_failed_makes_no_call() {
        let probe = Probe::bind();
        let channel = probe.channel();
        for (gate, bearer) in [
            (WriterGate { switch: false, ..all_good() }, Some("b")),
            (WriterGate { ledger: false, ..all_good() }, Some("b")),
            (WriterGate { zdr: true, ..all_good() }, Some("b")),
            (WriterGate { waived: true, ..all_good() }, Some("b")),
            (WriterGate { space: true, ..all_good() }, Some("b")),
            (WriterGate { depth: 1, ..all_good() }, Some("b")),
            (all_good(), None),
        ] {
            let mut writer = FoldWriter::seeded(&channel, bearer, "run-1", gate, 4, 2);
            let (result, calls) = park_through(&mut writer, parkable_fold(), Vec::new()).await;
            assert_eq!(
                result,
                Err(ParkSaveError::Save(SaveError::Unavailable { ambiguous: false })),
                "{gate:?}"
            );
            assert!(calls.is_empty(), "{gate:?}");
            // The real entry point makes no connection either.
            let result = writer
                .save_park(parkable_fold(), ParkReason::ProviderExhausted, due())
                .await;
            assert_eq!(
                result,
                Err(ParkSaveError::Save(SaveError::Unavailable { ambiguous: false }))
            );
            assert_eq!(writer.expected_seq(), 4, "nothing moved");
        }
        let mut failed = FoldWriter::seeded(&channel, Some("b"), "run-1", all_good(), 4, 2);
        failed.apply(SaveError::Closed);
        assert!(failed.is_failed());
        let result = failed
            .save_park(parkable_fold(), ParkReason::ProviderExhausted, due())
            .await;
        assert_eq!(
            result,
            Err(ParkSaveError::Save(SaveError::Unavailable { ambiguous: false }))
        );
        assert_eq!(probe.connections(), 0, "no writer that is not active reached the store");
    }

    /// Only a stale or a closed answer is a lost fence. A refusal for good fails the writer too,
    /// but nothing else holds the fold on its account; an unavailable or switched-off store does
    /// not fail the writer at all.
    #[tokio::test]
    async fn only_a_stale_or_closed_answer_is_a_lost_fence() {
        let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
        for (error, lost) in [
            (SaveError::Stale, true),
            (SaveError::Closed, true),
            (SaveError::Refused(RefusedBy::Denied), false),
            (SaveError::Unavailable { ambiguous: true }, false),
            (SaveError::StoreOff, false),
        ] {
            let mut writer = FoldWriter::new(&channel, Some("b"), "run-1", all_good());
            assert!(writer.is_active(), "{error:?}: the control writes");
            writer.apply(error);
            assert_eq!(writer.lost_fence(), lost, "{error:?}");
        }
        let writer = FoldWriter::new(&channel, Some("b"), "run-1", all_good());
        assert!(!writer.lost_fence(), "an active writer holds no lost fence");
    }

    #[test]
    fn the_prompt_digest_of_a_run_is_the_shared_digest_of_prompt_and_tools() {
        let tools = golden_tools();
        let expected = lf::prompt_digest(
            "SYSTEM_PROMPT_GOLDEN",
            &[
                (
                    "knowledge_search",
                    "Search the org knowledge base.",
                    r#"{"type":"object","properties":{"query":{"type":"string"}}}"#,
                ),
                (
                    "delete_records",
                    "Delete records (destructive).",
                    r#"{"type":"object","properties":{}}"#,
                ),
            ],
        );
        assert_eq!(prompt_digest_of("SYSTEM_PROMPT_GOLDEN", &tools), expected);
        let mut reversed = tools;
        reversed.reverse();
        assert_eq!(
            prompt_digest_of("SYSTEM_PROMPT_GOLDEN", &reversed),
            expected,
            "order is not part of it"
        );
        assert_ne!(prompt_digest_of("another prompt", &reversed), expected);
    }

    /// The writer gate and the pause site, driven by the posture matrix.
    /// `tests/loop_fold_zdr_matrix.rs` proves the SHARED gate over every row but no
    /// production code calls it, so this is the test that puts the same rows through the
    /// gates this crate really has: for every enforcement row the resolver serves, the
    /// flags the run hands `WriterGate` (`zdr` from the request, `waived` from the route,
    /// as `run_agent` reads them) must leave the writer on for a persistent row and off,
    /// for the right reason, for a ZDR or a waived one. A waived row has `zdr == false`,
    /// so a gate that looked at `zdr` alone would write a fold for it. The pause site is
    /// held to the same rows through `run_pause_mode`, fed the expression `agent.rs`
    /// passes it.
    #[test]
    fn the_writer_gate_and_the_pause_mint_follow_the_posture_matrix_row_by_row() {
        use mp_contracts::retention::{
            resolve_turn, ClaimsView, Resolution, RouteView, WaiverView,
        };
        const MATRIX: &str = include_str!("../../tests/fixtures/zdr-posture-matrix.json");
        let matrix: serde_json::Value = serde_json::from_str(MATRIX).expect("the matrix is JSON");
        let _continue = crate::runtime_loop::resume::ContinueSwitch::set(true);
        let (mut open, mut zdr_off, mut waived_off) = (0_usize, 0_usize, 0_usize);
        for row in matrix["enforcement_rows"].as_array().expect("rows") {
            let id = row["id"].as_str().unwrap_or("?");
            let claims: Vec<ClaimsView<'_>> = row["inputs"]["bearers"]
                .as_array()
                .expect("bearers")
                .iter()
                .map(|bearer| ClaimsView {
                    zdr: bearer["zdr"].as_bool().expect("zdr"),
                    org_zdr: bearer["org_zdr"].as_bool(),
                    waiver: (!bearer["waiver"].is_null()).then(|| WaiverView {
                        version: u32::try_from(bearer["waiver"]["version"].as_u64().expect("v"))
                            .expect("fits"),
                        provider: bearer["waiver"]["provider"].as_str().expect("provider"),
                        connection: bearer["waiver"]["connection"].as_str().expect("connection"),
                    }),
                })
                .collect();
            let route = RouteView {
                provider: row["inputs"]["route"]["provider"]
                    .as_str()
                    .expect("provider"),
                connection: row["inputs"]["route"]["connection"].as_str(),
                explicit_zdr: row["inputs"]["route"]["explicit_zdr"]
                    .as_bool()
                    .expect("zdr"),
            };
            // What the run knows about itself: a refused turn never becomes a run.
            let (zdr, waived, expected) = match resolve_turn(&claims, &route) {
                Resolution::Refuse(_) => continue,
                Resolution::Persistent { .. } => (false, false, None),
                Resolution::Zdr { .. } => (true, false, Some(OffReason::Zdr)),
                Resolution::Waived { .. } => (false, true, Some(OffReason::Waived)),
            };
            let gate = WriterGate {
                zdr,
                waived,
                ..all_good()
            };
            assert_eq!(gate.decide(), expected, "{id}: the fold writer's gate");
            // The pause site (agent.rs): `!req.zdr && !ctx.route.is_waived()`.
            assert_eq!(
                crate::runtime_loop::resume::run_pause_mode(
                    !zdr && !waived,
                    "",
                    "",
                    "delete_records"
                ),
                if expected.is_none() { "loop" } else { "" },
                "{id}: the pause site's loop mint"
            );
            match expected {
                None => open += 1,
                Some(OffReason::Zdr) => zdr_off += 1,
                Some(_) => waived_off += 1,
            }
        }
        assert_eq!(
            (open, zdr_off, waived_off),
            (3, 4, 2),
            "every served row was checked"
        );
    }
}
