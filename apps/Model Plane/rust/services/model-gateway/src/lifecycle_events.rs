//! Counts-only frames for context the gateway changed without saying so (build plan WS15 S2).
//!
//! Each builder turns one occurrence the gateway already handles into one typed frame: a
//! history shed after a provider rejected the prompt, a history compaction, skill guidance
//! cut to its budget, or a Do run that cleared old tool results. The frames carry counts and
//! closed tokens from `mp_contracts::lifecycle_notice`, never text. `sse.rs` decides whether
//! a frame is sent, so with the switch off nothing here reaches a stream.

use crate::compaction::HeadSummary;
use crate::sse_events::ChatEvent;

/// What one history compaction did. A count is `None` where the path did not measure it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompactionFacts {
    outcome: &'static str,
    messages_compacted: Option<u32>,
    messages_kept: Option<u32>,
}

impl CompactionFacts {
    /// The `compaction` frame for these facts, scoped to the conversation history.
    #[must_use]
    pub fn event(self) -> ChatEvent {
        ChatEvent::Compaction {
            scope: "history",
            outcome: self.outcome,
            messages_compacted: self.messages_compacted,
            messages_kept: self.messages_kept,
            pinned_kept: None,
        }
    }
}

/// The facts of a history compaction over `total` messages, or `None` when nothing was
/// compacted. A cut head is `summarized` when the summarizer answered and `truncated` when
/// it did not: the head is then gone, replaced by a notice. A stored summary reused without
/// any cut is `reused`, so a client never reads a carried-over summary as a fresh one.
#[must_use]
pub fn compaction_facts(
    reused: bool,
    head: Option<HeadSummary>,
    total: usize,
    summary: Option<&str>,
) -> Option<CompactionFacts> {
    match head {
        Some(head) => {
            let summarized = summary.is_some_and(|text| !text.trim().is_empty());
            Some(CompactionFacts {
                outcome: if summarized { "summarized" } else { "truncated" },
                messages_compacted: Some(count(head.end.saturating_sub(head.start))),
                messages_kept: Some(count(total.saturating_sub(head.end))),
            })
        }
        None => reused.then_some(CompactionFacts {
            outcome: "reused",
            messages_compacted: None,
            messages_kept: None,
        }),
    }
}

/// The frame for a Do run whose prompt cleared its older tool results to fit the window.
#[must_use]
pub fn tool_results_cleared() -> ChatEvent {
    ChatEvent::Compaction {
        scope: "tool_results",
        outcome: "cleared",
        messages_compacted: None,
        messages_kept: None,
        pinned_kept: None,
    }
}

/// The frame for a provider-rejected prompt that dropped `dropped` older messages before it
/// was sent again.
#[must_use]
pub fn history_shed(dropped: usize) -> ChatEvent {
    ChatEvent::ContextChange {
        change: "history_shed",
        messages_dropped: Some(count(dropped)),
        skills_truncated: None,
        skills_dropped: None,
        layer: None,
        state: None,
    }
}

/// The frame for skill guidance cut to fit its budget: how many blocks were shortened and how
/// many were left out. `None` when the guidance fitted as it was.
#[must_use]
pub fn skills_truncated(truncated: usize, dropped: usize) -> Option<ChatEvent> {
    (truncated > 0 || dropped > 0).then(|| ChatEvent::ContextChange {
        change: "skills_truncated",
        messages_dropped: None,
        skills_truncated: Some(count(truncated)),
        skills_dropped: Some(count(dropped)),
        layer: None,
        state: None,
    })
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
