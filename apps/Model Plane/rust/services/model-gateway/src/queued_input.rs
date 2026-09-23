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
//! probe which runs exist. In-memory and single-replica for the same reason —
//! the SSE connection pins the run to one process, so only that process can
//! consume the queue. A multi-replica deployment needs the enqueue routed to the
//! replica holding the stream (a NATS subject keyed by request id), exactly the
//! promotion the cancel registry's note describes.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;

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

/// What happened to an enqueue attempt.
///
/// Every arm is distinguishable on purpose: the SPA has to tell "the run ended
/// before your message landed" (retry as a normal send) from "you have too many
/// pending" (wait) from "not your run" (nothing to do). Collapsing them into a
/// bool is what made the original drop invisible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueOutcome {
    /// Accepted. `pending` counts everything now waiting, including this one.
    Queued { pending: usize, thread_id: String, conversation_only: bool },
    /// No active stream owned by this tenant/user has that request id.
    NoActiveStream,
    /// [`MAX_QUEUED_MESSAGES`] already waiting.
    Full { pending: usize },
    /// Longer than [`MAX_QUEUED_CHARS`].
    TooLong { chars: usize },
    /// Nothing but whitespace.
    Empty,
}

struct QueuedEntry {
    queue: Arc<Mutex<VecDeque<String>>>,
    org_id: String,
    user_id: String,
    thread_id: String,
    conversation_only: bool,
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
    pub fn register(&self, request_id: &str, org_id: &str, user_id: &str, thread_id: &str, conversation_only: bool) {
        self.inner.insert(
            request_id.to_owned(),
            QueuedEntry {
                queue: Arc::new(Mutex::new(VecDeque::new())),
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                thread_id: thread_id.to_owned(),
                conversation_only,
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
        let entry = self.inner.get(request_id)?;
        if entry.org_id != org_id || entry.user_id != user_id {
            return None;
        }
        Some(entry.thread_id.clone())
    }

    /// Queue `text` for delivery at the run's next tool-round boundary.
    ///
    /// The thread id comes back from the registry rather than from the caller:
    /// the durable append that follows must land on the thread the *stream* is
    /// on, and a client-supplied thread id would be an authority the client does
    /// not have.
    pub fn enqueue_for(
        &self,
        request_id: &str,
        org_id: &str,
        user_id: &str,
        text: &str,
    ) -> EnqueueOutcome {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return EnqueueOutcome::Empty;
        }
        let chars = trimmed.chars().count();
        if chars > MAX_QUEUED_CHARS {
            return EnqueueOutcome::TooLong { chars };
        }
        // Ownership is checked before the length rules would ever reveal
        // anything, and a mismatched owner is indistinguishable from an
        // inactive stream — same reasoning as `CancelRegistry::cancel_for`.
        let Some(entry) = self.inner.get(request_id) else {
            return EnqueueOutcome::NoActiveStream;
        };
        if entry.org_id != org_id || entry.user_id != user_id {
            return EnqueueOutcome::NoActiveStream;
        }
        let mut queue = match entry.queue.lock() {
            Ok(queue) => queue,
            // A poisoned lock means a previous holder panicked mid-drain. The
            // queue is advisory text, so recovering it is strictly better than
            // failing the user's message.
            Err(poisoned) => poisoned.into_inner(),
        };
        if queue.len() >= MAX_QUEUED_MESSAGES {
            return EnqueueOutcome::Full {
                pending: queue.len(),
            };
        }
        queue.push_back(trimmed.to_owned());
        EnqueueOutcome::Queued {
            pending: queue.len(),
            thread_id: entry.thread_id.clone(),
            conversation_only: entry.conversation_only,
        }
    }

    /// Take everything waiting for `request_id`, leaving the queue empty.
    ///
    /// Draining rather than peeking is what makes delivery exactly-once: the
    /// round boundary is the only consumer, and a message injected twice would
    /// read to the model as the user repeating themselves.
    #[must_use]
    pub fn drain(&self, request_id: &str) -> Vec<String> {
        let Some(entry) = self.inner.get(request_id) else {
            return Vec::new();
        };
        let mut queue = match entry.queue.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// Stop accepting mid-run input for a finished stream (idempotent).
    pub fn finish(&self, request_id: &str) {
        self.inner.remove(request_id);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> QueuedInputRegistry {
        let registry = QueuedInputRegistry::new();
        registry.register("req-1", "org-a", "user-a", "thread-1", false);
        registry
    }

    #[test]
    fn a_queued_message_comes_back_with_the_streams_own_thread() {
        let registry = registry();
        let outcome = registry.enqueue_for("req-1", "org-a", "user-a", "  hold on, use EUR  ");
        assert_eq!(
            outcome,
            EnqueueOutcome::Queued {
                pending: 1,
                thread_id: "thread-1".to_owned(),
                conversation_only: false,
            }
        );
        assert_eq!(registry.drain("req-1"), vec!["hold on, use EUR".to_owned()]);
    }

    #[test]
    fn queued_input_inherits_the_authorized_stream_source_scope() {
        let registry = QueuedInputRegistry::new();
        registry.register("isolated", "org-a", "user-a", "thread-1", true);
        assert_eq!(registry.enqueue_for("isolated", "org-a", "user-a", "shorten the draft"),
            EnqueueOutcome::Queued { pending: 1, thread_id: "thread-1".to_owned(), conversation_only: true });
        assert_eq!(registry.enqueue_for("isolated", "org-b", "user-a", "widen sources"), EnqueueOutcome::NoActiveStream);
    }

    /// The whole point: nothing is silently dropped. An unknown or foreign
    /// request id must SAY so, because the SPA's fallback for that case is to
    /// send the message as a normal turn instead.
    #[test]
    fn an_unknown_or_foreign_run_is_reported_not_swallowed() {
        let registry = registry();
        assert_eq!(
            registry.enqueue_for("req-nope", "org-a", "user-a", "hi"),
            EnqueueOutcome::NoActiveStream
        );
        assert_eq!(
            registry.enqueue_for("req-1", "org-b", "user-a", "hi"),
            EnqueueOutcome::NoActiveStream,
            "another tenant must not be able to inject text into this run"
        );
        assert_eq!(
            registry.enqueue_for("req-1", "org-a", "user-b", "hi"),
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
                registry.enqueue_for("req-1", "org-a", "user-a", "more"),
                EnqueueOutcome::Queued { .. }
            ));
        }
        assert_eq!(
            registry.enqueue_for("req-1", "org-a", "user-a", "one too many"),
            EnqueueOutcome::Full {
                pending: MAX_QUEUED_MESSAGES
            }
        );
        let long = "x".repeat(MAX_QUEUED_CHARS + 1);
        assert_eq!(
            registry.enqueue_for("req-1", "org-a", "user-a", &long),
            EnqueueOutcome::TooLong {
                chars: MAX_QUEUED_CHARS + 1
            },
            "length is checked before the queue, so a too-long message never occupies a slot"
        );
        assert_eq!(
            registry.enqueue_for("req-1", "org-a", "user-a", "   "),
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
                registry.enqueue_for("req-1", "org-a", "user-a", &at_limit),
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
        registry.enqueue_for("req-1", "org-a", "user-a", "first");
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
            registry.enqueue_for("req-1", "org-a", "user-a", "too late"),
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
}
