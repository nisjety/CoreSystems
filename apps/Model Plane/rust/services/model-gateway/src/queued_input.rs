//! Mid-run user input, delivered at a tool-round boundary.
//!
//! # The bug this replaces
//!
//! The SPA's send path began `if (!content || state.status === 'streaming')
//! return` — a message typed while the agent was working was **silently
//! dropped**. Not queued, not refused, not shown as rejected: gone. The user
//! watched their words disappear and had to retype them after the turn ended.
//!
//! # Why a boundary rather than "now" or "after"
//!
//! pi offers `steer` (inject after the current tool round) and `followUp` (wait
//! for a natural stop) and makes the *caller* pick. The caller cannot know:
//! whether a message redirects the work or merely follows it is a property of
//! what it says, which only the model reading it can judge.
//!
//! So delivery is at the round boundary — the one place the loop is between
//! actions rather than mid-call — and it arrives as a **pause**: the text, plus
//! [`QUEUED_INPUT_PAUSE`] telling the model to classify it and act accordingly.
//! A redirect gets acted on immediately; a follow-up waits for the natural stop
//! without being forgotten. The model decides, from the content.
//!
//! # Shape
//!
//! Deliberately the same shape as [`crate::cancel_registry`]: a per-`request_id`
//! entry bound to the authenticated tenant and user at registration, so a
//! request id cannot be used to inject text into another tenant's run or to
//! probe which runs exist. In-memory: the SSE connection pins the run to one
//! process, so only that process can consume the queue. With the run-control bus
//! on, an enqueue that lands on another replica is routed to the owner
//! ([`crate::run_control::RunControl::queue_input`]); the owner applies it to this
//! same registry, so the bounds, the ownership check and the order are one code
//! path whichever replica the user reached.
//!
//! # Send receipts
//!
//! With `MODEL_GATEWAY_SEND_RECEIPTS` on, a message is stored in the transcript under its
//! `client_message_id` before it is queued, and that id is the queue's `op_id`: a repeat after a
//! lost reply is queued once, and the loop takes each message with its id
//! ([`QueuedInputRegistry::drain_receipted`]) so the delivery event can name what it carries.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use mp_contracts::zdr_refusal::Refusal;
use serde::{Deserialize, Serialize};

use crate::auth::Claims;
use crate::normalize::RetrievalScope;
use crate::run_control::{RunControl, RunControlError, RunControlMsg, RunControlOp, RunControlSubject};
use crate::zdr_posture::{decide_for, Route, TurnPosture};

/// The instruction that turns delivery into a pause rather than an interruption.
///
/// Both failure modes are named explicitly because both were observed in
/// harnesses that inject mid-run text with no framing: the model either drops
/// the message (it looks like context, not a request) or stops to ask which
/// kind it is, stalling the run on a question the text already answers.
pub const QUEUED_INPUT_PAUSE: &str = "\
[The user sent this while you were working, so it is newer than everything above \
and they have not seen your latest output. Decide which it is before continuing:

- It changes what you should be doing. Adapt now — drop or adjust the current \
step, and say in one line what changed.
- It is a follow-up that comes after the task you are on. Keep going, finish \
that task, and address this before you end your turn.

Do not ask the user which of the two it is — decide from what they wrote. Do not \
restart work you have already completed, and do not leave the message \
unanswered.]";

/// Per message. A mid-run injection spends prompt budget the turn never planned
/// for, and the turn may already be near the compaction threshold.
pub const MAX_QUEUED_CHARS: usize = 2_000;

/// Per run. Past a few, the user is not steering — they are typing at a wall,
/// and delivering all of it would bury the task the run is actually on.
pub const MAX_QUEUED_MESSAGES: usize = 3;

/// The switch for durable send receipts (WS04 S6). Off, a queued message is enqueued first and
/// persisted after, and a send names no message to the transcript; on, a message is persisted
/// under its `client_message_id` before it is delivered, and the id is the receipt the SPA's tray
/// reports against. Strict: only the literal `true` turns it on.
pub const SEND_RECEIPTS_ENV: &str = "MODEL_GATEWAY_SEND_RECEIPTS";

/// The strict parser of [`SEND_RECEIPTS_ENV`]: `true` in any case with surrounding whitespace, and
/// nothing else, so a typo never turns receipts on.
#[must_use]
pub fn send_receipts_enabled_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// Whether durable send receipts are on, read from the environment at each call.
#[must_use]
pub fn send_receipts_enabled() -> bool {
    send_receipts_enabled_from(std::env::var(SEND_RECEIPTS_ENV).ok().as_deref())
}

/// Whether `id` can be a `client_message_id`: the shape session-core's `messages` CHECK allows.
/// It rides the run-control bus as the message's `op_id`, in the body and never in a subject.
#[must_use]
pub fn valid_client_message_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_alphanumeric())
        && id.len() <= 128
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// How many accepted `op_id`s a run remembers. One per lost reply is all a repeat needs, and a
/// run takes at most [`MAX_QUEUED_MESSAGES`] at a time, so this is generous without being a
/// store.
const MAX_SEEN_OPS: usize = 16;

/// What happened to an enqueue attempt.
///
/// Every arm is distinguishable on purpose: the SPA has to tell "the run ended
/// before your message landed" (retry as a normal send) from "you have too many
/// pending" (wait) from "not your run" (nothing to do). Collapsing them into a
/// bool is what made the original drop invisible.
///
/// Serialised on the run-control bus as the owner's answer to a queue request, so a message
/// queued from another replica comes back as the same arm it would have here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnqueueOutcome {
    /// Accepted. `pending` counts everything now waiting, including this one.
    Queued {
        pending: usize,
        thread_id: String,
        conversation_only: bool,
        /// The stream's own retrieval scope (chat-wave2 scope spec §5's
        /// mid-run rule): a message queued mid-stream always inherits the
        /// scope the run STARTED with, same as `conversation_only` already
        /// does above — a picker change mid-stream only takes effect on the
        /// next turn, never the in-flight one.
        #[serde(with = "retrieval_scope_wire")]
        retrieval_scope: RetrievalScope,
    },
    /// The caller named a thread other than the one the stream is on. Decided before anything
    /// is queued: a message that fails its authority check must not still reach the model.
    ThreadMismatch { streaming_thread_id: String },
    /// No active stream owned by this tenant/user has that request id.
    NoActiveStream,
    /// [`MAX_QUEUED_MESSAGES`] already waiting.
    Full { pending: usize },
    /// Longer than [`MAX_QUEUED_CHARS`].
    TooLong { chars: usize },
    /// Nothing but whitespace.
    Empty,
}

/// The `retrieval_scope` field of [`EnqueueOutcome::Queued`] on the wire. `RetrievalScope` has no
/// serde of its own (it is a request-time axis, not a contract), so the three labels live here,
/// and an unknown label is an error rather than a guess.
mod retrieval_scope_wire {
    use super::RetrievalScope;
    use serde::{Deserialize, Deserializer, Serializer};

    // serde's `with` hands the field by reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn serialize<S: Serializer>(scope: &RetrievalScope, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(match scope {
            RetrievalScope::Auto => "auto",
            RetrievalScope::KnowledgeOnly => "knowledge_only",
            RetrievalScope::WebOnly => "web_only",
        })
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<RetrievalScope, D::Error> {
        match String::deserialize(input)?.as_str() {
            "auto" => Ok(RetrievalScope::Auto),
            "knowledge_only" => Ok(RetrievalScope::KnowledgeOnly),
            "web_only" => Ok(RetrievalScope::WebOnly),
            other => Err(serde::de::Error::custom(format!(
                "unknown retrieval scope {other:?}"
            ))),
        }
    }
}

/// What waits for a run, and which attempts it has already accepted.
#[derive(Default)]
struct QueueState {
    /// `(text, op_id)`, oldest first. With send receipts on, the op id is the message's
    /// `client_message_id`, which is what lets the delivery event name the messages it carries.
    pending: VecDeque<(String, String)>,
    /// `(op_id, pending count answered)` of recently accepted enqueues, oldest first. A repeat
    /// of one (the caller's reply was lost, so it asked again) is answered the same and queues
    /// nothing, so a retry can never deliver a message twice.
    seen_ops: VecDeque<(String, usize)>,
}

/// One message the loop takes at a round boundary, with the id its sender gave it (empty when
/// none) so the delivery can be reported against the message the user sees in the tray.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub text: String,
    pub client_message_id: String,
}

/// Why `text` can never be queued, whatever run it is aimed at. Checked before a message is
/// persisted as well as before it is queued, so a message that is refused for its own shape
/// leaves no trace in the transcript.
#[must_use]
pub fn bounds_refusal(text: &str) -> Option<EnqueueOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some(EnqueueOutcome::Empty);
    }
    let chars = trimmed.chars().count();
    (chars > MAX_QUEUED_CHARS).then_some(EnqueueOutcome::TooLong { chars })
}

struct QueuedEntry {
    queue: Mutex<QueueState>,
    org_id: String,
    user_id: String,
    thread_id: String,
    conversation_only: bool,
    retrieval_scope: RetrievalScope,
}

/// Tracks the mid-run input queue for each active stream, by `request_id`.
#[derive(Clone, Default)]
pub struct QueuedInputRegistry {
    inner: Arc<DashMap<String, QueuedEntry>>,
}

impl QueuedInputRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start accepting mid-run input for `request_id`. Call
    /// [`finish`](Self::finish) when the stream ends.
    pub fn register(
        &self,
        request_id: &str,
        org_id: &str,
        user_id: &str,
        thread_id: &str,
        conversation_only: bool,
        retrieval_scope: RetrievalScope,
    ) {
        self.inner.insert(
            request_id.to_owned(),
            QueuedEntry {
                queue: Mutex::new(QueueState::default()),
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                thread_id: thread_id.to_owned(),
                conversation_only,
                retrieval_scope,
            },
        );
    }

    /// The thread an active stream is on, for an owner who may see it.
    ///
    /// Exists so a caller can reject a mismatched claim BEFORE anything is
    /// queued: refusing after the enqueue would leave a message that fails its
    /// authority check but still reaches the model.
    #[must_use]
    pub fn thread_for(&self, request_id: &str, org_id: &str, user_id: &str) -> Option<String> {
        self.stream_for(request_id, org_id, user_id)
            .map(|(thread_id, _)| thread_id)
    }

    /// [`thread_for`](Self::thread_for) with the stream's `conversation_only` scope, which a
    /// message persisted ahead of its delivery has to be written under.
    #[must_use]
    pub fn stream_for(&self, request_id: &str, org_id: &str, user_id: &str) -> Option<(String, bool)> {
        let entry = self.inner.get(request_id)?;
        if entry.org_id != org_id || entry.user_id != user_id {
            return None;
        }
        Some((entry.thread_id.clone(), entry.conversation_only))
    }

    /// Queue `text` for delivery at the run's next tool-round boundary, after the checks that
    /// must come first: the caller owns the stream, and the thread it names (when it names one)
    /// is the stream's.
    ///
    /// The thread id comes back from the registry rather than from the caller:
    /// the durable append that follows must land on the thread the *stream* is
    /// on, and a client-supplied thread id would be an authority the client does
    /// not have. A claimed thread is only compared with it, and a mismatch is
    /// refused before the text is queued ([`EnqueueOutcome::ThreadMismatch`]).
    ///
    /// `op_id` is the caller's id for this attempt (empty for none): a repeat of one this run
    /// already accepted returns the first answer and queues nothing.
    pub fn enqueue_checked(
        &self,
        request_id: &str,
        org_id: &str,
        user_id: &str,
        text: &str,
        claimed_thread_id: Option<&str>,
        op_id: &str,
    ) -> EnqueueOutcome {
        // A mismatched owner is indistinguishable from an inactive stream — same reasoning as
        // `CancelRegistry::cancel_for` — so an entry is only looked at for its own caller.
        let entry = self
            .inner
            .get(request_id)
            .filter(|entry| entry.org_id == org_id && entry.user_id == user_id);
        let claimed = claimed_thread_id
            .map(str::trim)
            .filter(|claimed| !claimed.is_empty());
        if let (Some(entry), Some(claimed)) = (&entry, claimed) {
            if claimed != entry.thread_id {
                return EnqueueOutcome::ThreadMismatch {
                    streaming_thread_id: entry.thread_id.clone(),
                };
            }
        }
        if let Some(refusal) = bounds_refusal(text) {
            return refusal;
        }
        let trimmed = text.trim();
        let Some(entry) = entry else {
            return EnqueueOutcome::NoActiveStream;
        };
        let mut state = match entry.queue.lock() {
            Ok(state) => state,
            // A poisoned lock means a previous holder panicked mid-drain. The
            // queue is advisory text, so recovering it is strictly better than
            // failing the user's message.
            Err(poisoned) => poisoned.into_inner(),
        };
        let queued = |pending| EnqueueOutcome::Queued {
            pending,
            thread_id: entry.thread_id.clone(),
            conversation_only: entry.conversation_only,
            retrieval_scope: entry.retrieval_scope,
        };
        if !op_id.is_empty() {
            if let Some((_, pending)) = state.seen_ops.iter().find(|(seen, _)| seen == op_id) {
                return queued(*pending);
            }
        }
        if state.pending.len() >= MAX_QUEUED_MESSAGES {
            return EnqueueOutcome::Full {
                pending: state.pending.len(),
            };
        }
        state.pending.push_back((trimmed.to_owned(), op_id.to_owned()));
        let pending = state.pending.len();
        if !op_id.is_empty() {
            if state.seen_ops.len() >= MAX_SEEN_OPS {
                state.seen_ops.pop_front();
            }
            state.seen_ops.push_back((op_id.to_owned(), pending));
        }
        queued(pending)
    }

    /// Take everything waiting for `request_id`, leaving the queue empty.
    ///
    /// Draining rather than peeking is what makes delivery exactly-once: the
    /// round boundary is the only consumer, and a message injected twice would
    /// read to the model as the user repeating themselves.
    #[must_use]
    pub fn drain(&self, request_id: &str) -> Vec<String> {
        self.drain_receipted(request_id)
            .into_iter()
            .map(|message| message.text)
            .collect()
    }

    /// [`drain`](Self::drain), with each message's `client_message_id`. The same take-everything,
    /// leave-nothing contract: it is the one consumer, so a delivery is exactly once.
    #[must_use]
    pub fn drain_receipted(&self, request_id: &str) -> Vec<QueuedMessage> {
        let Some(entry) = self.inner.get(request_id) else {
            return Vec::new();
        };
        let mut state = match entry.queue.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state
            .pending
            .drain(..)
            .map(|(text, client_message_id)| QueuedMessage { text, client_message_id })
            .collect()
    }

    /// Stop accepting mid-run input for a finished stream (idempotent). Returns how many
    /// accepted messages the loop never took, so the caller can count and log them: the user
    /// was told those were queued.
    pub fn finish(&self, request_id: &str) -> usize {
        self.inner.remove(request_id).map_or(0, |(_, entry)| {
            let state = match entry.queue.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
            state.pending.len()
        })
    }

    /// Number of streams currently accepting mid-run input.
    #[must_use]
    pub fn active(&self) -> usize {
        self.inner.len()
    }
}

/// The messages to splice into the prompt for a delivery.
///
/// The pause note is its own `system` message and the user's words stay in
/// `user` messages, unedited. Wrapping the text in harness prose would put words
/// in the user's mouth — and the model would have no way to tell which part the
/// user actually wrote.
#[must_use]
pub fn delivery_messages(queued: &[String]) -> Vec<mp_contracts::model_plane::v1::ChatMessage> {
    use mp_contracts::model_plane::v1::ChatMessage;

    if queued.is_empty() {
        return Vec::new();
    }
    let mut messages = Vec::with_capacity(queued.len() + 1);
    messages.push(ChatMessage {
        compaction_summary: String::new(),
        role: "system".to_owned(),
        content: QUEUED_INPUT_PAUSE.to_owned(),
        name: String::new(),
    });
    messages.extend(queued.iter().map(|text| ChatMessage {
        compaction_summary: String::new(),
        role: "user".to_owned(),
        content: text.clone(),
        name: String::new(),
    }));
    messages
}

/// The run-control message that carries a queued message to the replica that owns the stream.
///
/// This is the only place the text is put into a [`RunControlMsg`], and it refuses a ZDR token
/// and a waived one first: ZDR turns keep nothing, and a waived run keeps none of its own, so
/// neither may put a message on a shared bus or into a stored run. The handler has already
/// refused them before it gets here; building the message checks again, so a caller that skipped
/// the handler's gate still cannot put the text on the wire.
///
/// # Errors
/// The typed refusal for a ZDR or waived token (or any token the posture decision refuses).
pub fn bus_message(
    claims: &Claims,
    request_id: &str,
    text: &str,
    claimed_thread_id: Option<&str>,
    op_id: &str,
) -> Result<RunControlMsg, Refusal> {
    match decide_for(claims, &Route::plain(false)) {
        Ok(TurnPosture::Persistent) => {}
        Ok(TurnPosture::Zdr { .. }) => return Err(Refusal::DURABLE_MUTATION_FORBIDDEN),
        Ok(TurnPosture::Waived { .. }) => return Err(Refusal::WAIVER_ROUTE_MISMATCH),
        Err(refusal) => return Err(refusal),
    }
    let mut msg = RunControlMsg::new(RunControlOp::Queue, &claims.org_id, &claims.user_id);
    request_id.clone_into(&mut msg.request_id);
    op_id.clone_into(&mut msg.op_id);
    msg.text = Some(text.to_owned());
    msg.claimed_thread_id = claimed_thread_id.map(str::to_owned);
    Ok(msg)
}

/// Where the stream a message is aimed at runs, and the thread it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamThread {
    /// On this replica, with the stream's own source scope.
    Here {
        thread_id: String,
        conversation_only: bool,
    },
    /// On another replica, which said so over the run-control bus. Its source scope is not on
    /// the bus.
    Peer(String),
    /// No stream of this caller is live anywhere: it ended, never ran, or is somebody else's.
    Gone,
    /// The bus could not say, so the stream may or may not be live.
    Unavailable,
}

/// The thread of the caller's stream `request_id`, learned before anything is written: a message
/// is persisted to the thread the *stream* is on (never one the client names) and before it is
/// delivered, so the thread has to be known first. The local registry answers first; only a
/// stream that is not here is asked about, over the bus, and only when the bus is on.
pub async fn stream_thread(
    queued_inputs: &QueuedInputRegistry,
    control: &RunControl,
    request_id: &str,
    org_id: &str,
    user_id: &str,
) -> StreamThread {
    if let Some((thread_id, conversation_only)) = queued_inputs.stream_for(request_id, org_id, user_id) {
        return StreamThread::Here { thread_id, conversation_only };
    }
    let Ok(subject) = RunControlSubject::request(request_id, RunControlOp::Probe) else {
        return StreamThread::Gone;
    };
    let mut msg = RunControlMsg::new(RunControlOp::Probe, org_id, user_id);
    request_id.clone_into(&mut msg.request_id);
    match control.request(&subject, &msg).await {
        Ok(reply) if reply.found => reply.thread_id.map_or(StreamThread::Gone, StreamThread::Peer),
        Ok(_)
        | Err(
            RunControlError::NoOwner | RunControlError::Disabled | RunControlError::InvalidSubject,
        ) => StreamThread::Gone,
        Err(
            RunControlError::Timeout
            | RunControlError::BadReply
            | RunControlError::PayloadTooLarge
            | RunControlError::Unavailable(_),
        ) => StreamThread::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `enqueue_checked` with no claimed thread and no op id: the plain enqueue most cases test.
    fn enqueue(
        registry: &QueuedInputRegistry,
        request_id: &str,
        org_id: &str,
        user_id: &str,
        text: &str,
    ) -> EnqueueOutcome {
        registry.enqueue_checked(request_id, org_id, user_id, text, None, "")
    }

    fn registry() -> QueuedInputRegistry {
        let registry = QueuedInputRegistry::new();
        registry.register("req-1", "org-a", "user-a", "thread-1", false, RetrievalScope::Auto);
        registry
    }

    #[test]
    fn a_queued_message_comes_back_with_the_streams_own_thread() {
        let registry = registry();
        let outcome = enqueue(&registry, "req-1", "org-a", "user-a", "  hold on, use EUR  ");
        assert_eq!(
            outcome,
            EnqueueOutcome::Queued {
                pending: 1,
                thread_id: "thread-1".to_owned(),
                conversation_only: false,
                retrieval_scope: RetrievalScope::Auto,
            }
        );
        assert_eq!(registry.drain("req-1"), vec!["hold on, use EUR".to_owned()]);
    }

    #[test]
    fn queued_input_inherits_the_authorized_stream_source_scope() {
        let registry = QueuedInputRegistry::new();
        registry.register("isolated", "org-a", "user-a", "thread-1", true, RetrievalScope::Auto);
        assert_eq!(enqueue(&registry, "isolated", "org-a", "user-a", "shorten the draft"),
            EnqueueOutcome::Queued {
                pending: 1,
                thread_id: "thread-1".to_owned(),
                conversation_only: true,
                retrieval_scope: RetrievalScope::Auto,
            });
        assert_eq!(enqueue(&registry, "isolated", "org-b", "user-a", "widen sources"), EnqueueOutcome::NoActiveStream);
    }

    /// chat-wave2 scope spec §5's mid-run rule: a message queued mid-stream
    /// always inherits the STREAM's retrieval scope, exactly the way
    /// `conversation_only` already does above — a picker change mid-stream
    /// only takes effect on the next turn, never the in-flight one.
    #[test]
    fn queued_input_inherits_the_streams_own_retrieval_scope() {
        let registry = QueuedInputRegistry::new();
        registry.register("req-1", "org-a", "user-a", "thread-1", false, RetrievalScope::WebOnly);
        assert_eq!(
            enqueue(&registry, "req-1", "org-a", "user-a", "check the news"),
            EnqueueOutcome::Queued {
                pending: 1,
                thread_id: "thread-1".to_owned(),
                conversation_only: false,
                retrieval_scope: RetrievalScope::WebOnly,
            }
        );
    }

    /// The whole point: nothing is silently dropped. An unknown or foreign
    /// request id must SAY so, because the SPA's fallback for that case is to
    /// send the message as a normal turn instead.
    #[test]
    fn an_unknown_or_foreign_run_is_reported_not_swallowed() {
        let registry = registry();
        assert_eq!(
            enqueue(&registry, "req-nope", "org-a", "user-a", "hi"),
            EnqueueOutcome::NoActiveStream
        );
        assert_eq!(
            enqueue(&registry, "req-1", "org-b", "user-a", "hi"),
            EnqueueOutcome::NoActiveStream,
            "another tenant must not be able to inject text into this run"
        );
        assert_eq!(
            enqueue(&registry, "req-1", "org-a", "user-b", "hi"),
            EnqueueOutcome::NoActiveStream,
            "another user in the same org must not either"
        );
        assert!(
            registry.drain("req-1").is_empty(),
            "a refused enqueue must leave the queue untouched"
        );
    }

    /// A refused message is refused with its own reason. `Full` tells the SPA to
    /// wait; `NoActiveStream` tells it to send normally. One bool could not.
    #[test]
    fn the_bounds_are_stated_and_distinguishable() {
        let registry = registry();
        for _ in 0..MAX_QUEUED_MESSAGES {
            assert!(matches!(
                enqueue(&registry, "req-1", "org-a", "user-a", "more"),
                EnqueueOutcome::Queued { .. }
            ));
        }
        assert_eq!(
            enqueue(&registry, "req-1", "org-a", "user-a", "one too many"),
            EnqueueOutcome::Full {
                pending: MAX_QUEUED_MESSAGES
            }
        );
        let long = "x".repeat(MAX_QUEUED_CHARS + 1);
        assert_eq!(
            enqueue(&registry, "req-1", "org-a", "user-a", &long),
            EnqueueOutcome::TooLong {
                chars: MAX_QUEUED_CHARS + 1
            },
            "length is checked before the queue, so a too-long message never occupies a slot"
        );
        assert_eq!(
            enqueue(&registry, "req-1", "org-a", "user-a", "   "),
            EnqueueOutcome::Empty
        );
    }

    /// Counted in characters, not bytes: a Norwegian message at the boundary
    /// must not be refused for its diacritics.
    #[test]
    fn the_length_bound_counts_characters() {
        let registry = registry();
        let at_limit = "æ".repeat(MAX_QUEUED_CHARS);
        assert!(
            matches!(
                enqueue(&registry, "req-1", "org-a", "user-a", &at_limit),
                EnqueueOutcome::Queued { .. }
            ),
            "{MAX_QUEUED_CHARS} multi-byte characters is exactly at the limit, not over it"
        );
    }

    /// Delivery is exactly-once. A message injected twice reads to the model as
    /// the user repeating themselves — and would be acted on twice.
    #[test]
    fn draining_empties_the_queue() {
        let registry = registry();
        enqueue(&registry, "req-1", "org-a", "user-a", "first");
        assert_eq!(registry.drain("req-1").len(), 1);
        assert!(registry.drain("req-1").is_empty());
    }

    /// The thread lookup is a read of the same tenant-bound entry, so it must
    /// keep the same silence for a foreign caller — otherwise it becomes the
    /// oracle `enqueue_for` refuses to be.
    #[test]
    fn the_thread_lookup_is_tenant_bound_too() {
        let registry = registry();
        assert_eq!(
            registry.thread_for("req-1", "org-a", "user-a").as_deref(),
            Some("thread-1")
        );
        assert!(registry.thread_for("req-1", "org-b", "user-a").is_none());
        assert!(registry.thread_for("req-1", "org-a", "user-b").is_none());
        assert!(registry.thread_for("req-nope", "org-a", "user-a").is_none());
    }

    #[test]
    fn finishing_stops_accepting_and_is_idempotent() {
        let registry = registry();
        assert_eq!(registry.active(), 1);
        registry.finish("req-1");
        registry.finish("req-1");
        assert_eq!(registry.active(), 0);
        assert_eq!(
            enqueue(&registry, "req-1", "org-a", "user-a", "too late"),
            EnqueueOutcome::NoActiveStream
        );
    }

    /// Order is the order they were sent. A steer followed by a correction to
    /// that steer is meaningless reversed.
    #[test]
    fn delivery_preserves_send_order_and_keeps_the_users_words_intact() {
        let queued = vec!["use EUR".to_owned(), "actually NOK".to_owned()];
        let messages = delivery_messages(&queued);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, QUEUED_INPUT_PAUSE);
        assert_eq!(messages[1].role, "user");
        assert_eq!(
            messages[1].content, "use EUR",
            "the user's text must arrive unedited, with no harness prose wrapped around it"
        );
        assert_eq!(messages[2].content, "actually NOK");
    }

    #[test]
    fn no_queued_input_means_no_injected_messages() {
        assert!(
            delivery_messages(&[]).is_empty(),
            "an empty drain must not push a bare instruction with nothing to classify"
        );
    }

    /// The pause has to name BOTH branches and forbid the two observed failure
    /// modes, or it is just an interruption with extra words.
    #[test]
    fn the_pause_states_both_branches_and_forbids_stalling() {
        let pause = QUEUED_INPUT_PAUSE.to_lowercase();
        assert!(
            pause.contains("changes what you should be doing"),
            "the redirect branch must be stated"
        );
        assert!(
            pause.contains("follow-up"),
            "the continuation branch must be stated"
        );
        assert!(
            pause.contains("do not ask the user which"),
            "asking which branch it is stalls the run on a question the text already answers"
        );
        assert!(
            pause.contains("unanswered"),
            "the message must not be droppable — that is the bug this replaces"
        );
    }

    /// The claim is compared before anything is queued, and only for the caller who owns the
    /// stream: a stranger naming any thread learns nothing about which thread it is.
    #[test]
    fn a_claimed_thread_other_than_the_streams_is_refused_before_anything_is_queued() {
        let registry = registry();
        assert_eq!(
            registry.enqueue_checked("req-1", "org-a", "user-a", "hi", Some("thread-9"), ""),
            EnqueueOutcome::ThreadMismatch {
                streaming_thread_id: "thread-1".to_owned()
            }
        );
        assert!(registry.drain("req-1").is_empty(), "a refused claim queues nothing");
        for foreign in [("org-b", "user-a"), ("org-a", "user-b")] {
            assert_eq!(
                registry.enqueue_checked("req-1", foreign.0, foreign.1, "hi", Some("thread-9"), ""),
                EnqueueOutcome::NoActiveStream,
                "{foreign:?}: a mismatch must not be an oracle for the stream's thread"
            );
        }
        // The right thread, a blank claim and no claim all go through.
        for claimed in [Some("thread-1"), Some(" thread-1 "), Some("  "), None] {
            assert!(matches!(
                registry.enqueue_checked("req-1", "org-a", "user-a", "ok", claimed, ""),
                EnqueueOutcome::Queued { .. } | EnqueueOutcome::Full { .. }
            ));
        }
    }

    /// A repeat of an attempt this run already accepted (its reply was lost, so the caller
    /// asked again) is the same answer and one message.
    #[test]
    fn a_repeated_op_id_returns_the_same_item_and_queues_once() {
        let registry = registry();
        let first = registry.enqueue_checked("req-1", "org-a", "user-a", "use EUR", None, "op-1");
        let second = registry.enqueue_checked("req-1", "org-a", "user-a", "use EUR", None, "op-1");
        assert_eq!(first, second);
        assert_eq!(registry.drain("req-1"), vec!["use EUR".to_owned()]);

        // An empty op id names no attempt, so two plain messages are two messages.
        enqueue(&registry, "req-1", "org-a", "user-a", "a");
        enqueue(&registry, "req-1", "org-a", "user-a", "a");
        assert_eq!(registry.drain("req-1").len(), 2);
    }

    /// The repeat of an accepted op wins over the bound: the caller already has its message in.
    #[test]
    fn a_repeat_is_answered_even_when_the_queue_is_full_and_a_new_op_is_not() {
        let registry = registry();
        let mut firsts = Vec::new();
        for n in 0..MAX_QUEUED_MESSAGES {
            firsts.push(registry.enqueue_checked("req-1", "org-a", "user-a", "m", None, &format!("op-{n}")));
        }
        assert_eq!(
            registry.enqueue_checked("req-1", "org-a", "user-a", "m", None, "op-0"),
            firsts[0]
        );
        assert_eq!(
            registry.enqueue_checked("req-1", "org-a", "user-a", "m", None, "op-new"),
            EnqueueOutcome::Full { pending: MAX_QUEUED_MESSAGES }
        );
    }

    #[test]
    fn the_remembered_ops_are_bounded() {
        let registry = registry();
        registry.enqueue_checked("req-1", "org-a", "user-a", "m", None, "op-first");
        for n in 0..MAX_SEEN_OPS {
            let _ = registry.drain("req-1");
            registry.enqueue_checked("req-1", "org-a", "user-a", "m", None, &format!("op-{n}"));
        }
        let _ = registry.drain("req-1");
        // The oldest op was evicted, so its repeat is a new message: the bound is the price of
        // not being a store.
        assert_eq!(
            registry.enqueue_checked("req-1", "org-a", "user-a", "m", None, "op-first"),
            EnqueueOutcome::Queued {
                pending: 1,
                thread_id: "thread-1".to_owned(),
                conversation_only: false,
                retrieval_scope: RetrievalScope::Auto,
            }
        );
        let _ = registry.drain("req-1");
        // The newest is still remembered: its repeat queues nothing.
        let newest = format!("op-{}", MAX_SEEN_OPS - 1);
        registry.enqueue_checked("req-1", "org-a", "user-a", "m", None, &newest);
        assert!(registry.drain("req-1").is_empty());
    }

    /// What the owner replies and the other replica reads: every arm survives the wire.
    #[test]
    fn every_outcome_survives_the_wire_encoding() {
        for scope in [RetrievalScope::Auto, RetrievalScope::KnowledgeOnly, RetrievalScope::WebOnly] {
            let outcomes = [
                EnqueueOutcome::Queued {
                    pending: 2,
                    thread_id: "thread-1".to_owned(),
                    conversation_only: true,
                    retrieval_scope: scope,
                },
                EnqueueOutcome::ThreadMismatch {
                    streaming_thread_id: "thread-1".to_owned(),
                },
                EnqueueOutcome::NoActiveStream,
                EnqueueOutcome::Full { pending: 3 },
                EnqueueOutcome::TooLong { chars: 2001 },
                EnqueueOutcome::Empty,
            ];
            for outcome in outcomes {
                let wire = serde_json::to_string(&outcome).expect("serialize");
                assert_eq!(serde_json::from_str::<EnqueueOutcome>(&wire).expect("parse"), outcome, "{wire}");
            }
        }
        assert!(
            serde_json::from_str::<EnqueueOutcome>(
                r#"{"queued":{"pending":1,"thread_id":"t","conversation_only":false,"retrieval_scope":"everything"}}"#
            )
            .is_err(),
            "an unknown scope label is an error, not a guess"
        );
    }

    /// What was accepted and never taken is counted at the end of the stream.
    #[test]
    fn finishing_reports_the_messages_the_loop_never_took() {
        let registry = registry();
        enqueue(&registry, "req-1", "org-a", "user-a", "one");
        enqueue(&registry, "req-1", "org-a", "user-a", "two");
        assert_eq!(registry.drain("req-1").len(), 2);
        enqueue(&registry, "req-1", "org-a", "user-a", "three");
        assert_eq!(registry.finish("req-1"), 1);
        assert_eq!(registry.finish("req-1"), 0, "finishing twice reports nothing more");
        assert_eq!(registry.finish("req-nope"), 0);
    }

    fn claims(zdr: bool, waiver: Option<serde_json::Value>) -> Claims {
        let mut payload = serde_json::json!({
            "sub": "user-a",
            "iss": "https://auth.test/model",
            "aud": "model-gateway",
            "exp": 4_102_444_800_i64,
            "nbf": 1_700_000_000_i64,
            "org_id": "org-a",
            "user_id": "user-a",
            "principal_type": "user",
            "zdr": zdr,
        });
        if let Some(waiver) = waiver {
            payload["org_zdr"] = serde_json::json!(true);
            payload["zdr_waiver"] = waiver;
        }
        serde_json::from_value(payload).expect("a minted token's payload is valid Claims")
    }

    /// The text only goes into a bus message for a persistent token: ZDR keeps nothing and a
    /// waived run keeps none of its own, so neither may put a message on a shared bus.
    #[test]
    fn queue_bus_message_is_never_built_for_zdr_or_waived_claims() {
        let waiver = serde_json::json!({
            "version": 1,
            "provider": mp_contracts::agentic_subscription::PROVIDER_ID,
            "connection": "conn-1",
        });
        assert_eq!(
            bus_message(&claims(true, None), "req-1", "secret words", None, "op-1"),
            Err(Refusal::DURABLE_MUTATION_FORBIDDEN)
        );
        assert!(
            bus_message(&claims(false, Some(waiver)), "req-1", "secret words", None, "op-1").is_err(),
            "a waived token is refused too"
        );

        let msg = bus_message(&claims(false, None), "req-1", "plain words", Some("thread-1"), "op-1")
            .expect("a persistent token may queue");
        assert_eq!(msg.op, RunControlOp::Queue);
        assert_eq!(
            (msg.org_id.as_str(), msg.user_id.as_str(), msg.request_id.as_str(), msg.op_id.as_str()),
            ("org-a", "user-a", "req-1", "op-1")
        );
        assert_eq!(msg.text.as_deref(), Some("plain words"));
        assert_eq!(msg.claimed_thread_id.as_deref(), Some("thread-1"));
    }

    #[test]
    fn the_send_receipts_switch_is_strict_and_defaults_off() {
        assert!(!send_receipts_enabled_from(None));
        for off in ["", "false", "1", "yes", "on", "tru", "truee", "enabled"] {
            assert!(!send_receipts_enabled_from(Some(off)), "{off:?}");
        }
        for on in ["true", "TRUE", " True "] {
            assert!(send_receipts_enabled_from(Some(on)), "{on:?}");
        }
    }

    #[test]
    fn a_client_message_id_has_the_shape_session_core_stores() {
        for good in ["a", "0", "msg_01-X", &"a".repeat(128)] {
            assert!(valid_client_message_id(good), "{good}");
        }
        for bad in ["", "-a", "_a", "a b", "a.b", "a/b", "é", "a\n", &"a".repeat(129)] {
            assert!(!valid_client_message_id(bad), "{bad:?}");
        }
    }

    /// The delivery event names what it carries: the loop takes the ids with the texts, in order,
    /// and a message sent without one carries an empty id.
    #[test]
    fn a_drain_hands_back_each_message_with_its_client_message_id() {
        let registry = registry();
        registry.enqueue_checked("req-1", "org-a", "user-a", " first ", None, "msg-1");
        registry.enqueue_checked("req-1", "org-a", "user-a", "second", None, "");
        assert_eq!(
            registry.drain_receipted("req-1"),
            vec![
                QueuedMessage { text: "first".into(), client_message_id: "msg-1".into() },
                QueuedMessage { text: "second".into(), client_message_id: String::new() },
            ]
        );
        assert!(registry.drain_receipted("req-1").is_empty(), "a delivery is exactly once");
        // The plain drain is the same take, texts only.
        registry.enqueue_checked("req-1", "org-a", "user-a", "third", None, "msg-3");
        assert_eq!(registry.drain("req-1"), vec!["third".to_owned()]);
    }

    #[test]
    fn a_message_that_is_refused_for_its_shape_is_refused_before_anything_is_written() {
        assert_eq!(bounds_refusal("  \n "), Some(EnqueueOutcome::Empty));
        let long = "x".repeat(MAX_QUEUED_CHARS + 1);
        assert_eq!(
            bounds_refusal(&long),
            Some(EnqueueOutcome::TooLong { chars: MAX_QUEUED_CHARS + 1 })
        );
        assert_eq!(bounds_refusal(&"x".repeat(MAX_QUEUED_CHARS)), None);
        assert_eq!(bounds_refusal(" hi "), None);
    }

    fn replica(hub: &crate::run_control::LoopbackHub) -> RunControl {
        RunControl::loopback(hub.clone(), std::time::Duration::from_millis(200))
    }

    /// The thread is learned before the message is persisted: from the local registry, or from the
    /// replica that owns the stream, and never for somebody else's stream or one that ended.
    #[tokio::test]
    async fn the_thread_of_a_stream_is_learned_here_or_from_its_owner_and_never_for_a_stranger() {
        let hub = crate::run_control::LoopbackHub::new();
        let (owner, peer) = (replica(&hub), replica(&hub));
        let (owner_cancels, owner_queue) = (crate::cancel_registry::CancelRegistry::new(), registry());
        let peer_queue = QueuedInputRegistry::new();
        let _flag = owner_cancels.register_run("req-1", "org-a", "user-a", "run-1");
        assert!(owner.attach(&owner_cancels, &owner_queue, "req-1", "run-1").await);

        let ask = |queue: &QueuedInputRegistry, control: &RunControl, org: &str, user: &str| {
            let (queue, control, org, user) = (queue.clone(), control.clone(), org.to_owned(), user.to_owned());
            async move { stream_thread(&queue, &control, "req-1", &org, &user).await }
        };
        assert_eq!(
            ask(&owner_queue, &owner, "org-a", "user-a").await,
            StreamThread::Here { thread_id: "thread-1".into(), conversation_only: false }
        );
        assert_eq!(
            ask(&peer_queue, &peer, "org-a", "user-a").await,
            StreamThread::Peer("thread-1".into())
        );
        for (org, user) in [("org-b", "user-a"), ("org-a", "user-b")] {
            assert_eq!(ask(&peer_queue, &peer, org, user).await, StreamThread::Gone, "{org}/{user}");
            assert_eq!(ask(&owner_queue, &owner, org, user).await, StreamThread::Gone, "{org}/{user}");
        }

        // An owner that does not answer is not "gone": the message may belong to a live run.
        hub.mute(true);
        assert_eq!(ask(&peer_queue, &peer, "org-a", "user-a").await, StreamThread::Unavailable);
        hub.mute(false);

        // The stream ends: nobody owns it any more.
        owner_cancels.finish("req-1");
        assert_eq!(ask(&peer_queue, &peer, "org-a", "user-a").await, StreamThread::Gone);
        // With the bus off an unknown stream is gone, and nothing is asked.
        assert_eq!(
            ask(&peer_queue, &RunControl::off(), "org-a", "user-a").await,
            StreamThread::Gone
        );
    }
}
