//! Projection of one recalled memory onto what a person can be shown about it.
//!
//! # Why a projection and not a field mapping
//!
//! Ported from `deepseek-harness`'s `context-provenance.ts` (MIT), whose central
//! property is worth keeping exactly: the presentation is derived **from the
//! durable record alone**, and the reader keeps **no table of known producers**.
//! Its own comment puts it best — "a renamed or newly mounted producer must never
//! need a client release to stay identifiable, and a resumed or foreign log must
//! project the same way as a live one".
//!
//! Concretely that means a topic this build has never seen still projects to a
//! usable label instead of vanishing, and a `provenance` value added to the proto
//! after this build shipped degrades to "unrecorded" rather than being silently
//! read as something friendlier. The same no-allowlist rule already governs the
//! SSE relay and the buffered-frame store.
//!
//! # The one honesty rule the proto itself states
//!
//! `MEMORY_PROVENANCE_UNSPECIFIED` carries the comment "Pre-provenance rows land
//! here; **never render these as stated**". Rows written before provenance
//! existed are indistinguishable from ones the user dictated, so presenting them
//! as "you told me this" would manufacture consent the record does not support.
//! [`MemoryOrigin::Unrecorded`] is a distinct third state for exactly that, and
//! it is why this is not a two-valued boolean.

use mp_contracts::model_plane::v1::{MemoryEntry, MemoryProvenance};

/// Header on the recalled-memory system block.
///
/// # Why the framing carries weight
///
/// The block used to open with "Relevant memory:", which asserts two things the
/// recall cannot support: that the entries are relevant to THIS request, and
/// that they are established fact. Recall is a similarity search over everything
/// remembered about the user, so a new conversation gets the top few entries
/// whether or not anything matches.
///
/// What that produced, measured on 2026-09-14: a project plan for a customer
/// portal listed "Ekstra frakt ved dellevering (ordre FF-1042)" as an open
/// question — an order from an unrelated conversation three hours earlier,
/// recalled and written into a deliverable as if it belonged there.
///
/// So the block now says what these entries actually are (background about the
/// person, possibly irrelevant) and what they may be used for (matching language
/// and standing preferences), and states the one rule the failure needed: they
/// are not a source, and they do not enter a deliverable on their own.
pub const MEMORY_CONTEXT_HEADER: &str = concat!(
    "Background about the person you are talking to, remembered from earlier ",
    "conversations. This is context about THEM, not material for the current ",
    "task, and it may well be irrelevant to this request.\n",
    "Use it to match their working language, tone and standing preferences. Do ",
    "NOT treat it as a source: do not restate it as fact, and do not carry any ",
    "of it into a document, plan, report, draft or recommendation unless the ",
    "user's current request or the material they supplied refers to it. If it ",
    "conflicts with what the user says or supplies now, what they say now wins."
);

/// Build the system block that carries recalled memories into a prompt, or
/// `None` when nothing was recalled.
///
/// Shared by both chat paths (`sse` and `grpc`) so the two cannot drift into
/// framing the same entries differently.
#[must_use]
pub fn memory_context_block(entries: &[String]) -> Option<String> {
    let body = entries
        .iter()
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .map(|entry| format!("- {entry}"))
        .collect::<Vec<_>>()
        .join("\n");
    (!body.is_empty()).then(|| format!("{MEMORY_CONTEXT_HEADER}\n\n{body}"))
}

/// The role a recalled memory plays in the turn.
///
/// Mirrors `DeepSeek`'s `ContextRole` split: `Recall` is material lifted out of
/// another conversation, `Inject` is context that belongs to no conversation in
/// particular (an org policy, a workspace fact). The distinction is what lets a
/// reader tell "you told me this in another chat" from "this is how your
/// organization is configured" — two very different things to see quoted back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRole {
    /// Carried over from another conversation.
    Recall,
    /// Org/workspace/policy context that is not conversation material.
    Inject,
}

impl MemoryRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recall => "recall",
            Self::Inject => "inject",
        }
    }
}

/// How the memory came to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryOrigin {
    /// The user said it, or asked for it to be saved.
    Stated,
    /// A model inferred it from a conversation.
    Inferred,
    /// Not recorded. Distinct from `Stated` on purpose — see the module docs.
    Unrecorded,
}

impl MemoryOrigin {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stated => "stated",
            Self::Inferred => "inferred",
            Self::Unrecorded => "unrecorded",
        }
    }
}

/// What is shown for one recalled memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecalledMemoryView {
    /// Stable id, so a reader can act on the exact row (correct it, delete it).
    pub memory_id: String,
    pub role: MemoryRole,
    pub origin: MemoryOrigin,
    /// Human-facing name for the row header, taken from the record itself.
    pub label: String,
    /// Bounded excerpt of the content.
    pub preview: String,
}

/// Longest excerpt shown per recalled memory.
///
/// The point of surfacing recall is "can I see what you remember and correct
/// it", which a first line answers; pasting whole memories into every turn's
/// stream would make the chat unreadable and re-send content the reader already
/// has stored. Truncation is marked, never silent.
pub const MEMORY_PREVIEW_CHARS: usize = 160;

/// Most recalled memories described in one event.
///
/// The prefetch asks for 5. This is a wire ceiling so a future limit increase
/// cannot silently turn every turn's stream into a memory dump.
pub const MAX_DESCRIBED_MEMORIES: usize = 8;

const TRUNCATION_SUFFIX: &str = "…";

/// Label for a memory whose topic is absent.
///
/// Deliberately not the memory id: an id is not a name, and showing one in a
/// row header tells a reader nothing about what the row is.
const UNLABELLED: &str = "Minne";

/// Project one durable entry onto its presentation.
///
/// The entry's own owning thread decides the role: an entry owned by some
/// conversation is material recalled from one, and an entry owned by no
/// conversation is standing org-level context injected into every prompt.
/// Deliberately NOT a comparison against the current thread — an entry recalled
/// from this same conversation is still a recall, and reading the current thread
/// here would make the label depend on where it is displayed rather than on what
/// the record is.
#[must_use]
pub fn project_recalled_memory(entry: &MemoryEntry) -> RecalledMemoryView {
    let role = if entry.thread_id.trim().is_empty() {
        // No owning conversation: an org/workspace/policy fact.
        MemoryRole::Inject
    } else {
        MemoryRole::Recall
    };

    // `MemoryProvenance::try_from` fails for a value added to the proto after
    // this build. Unknown must land on `Unrecorded`, never on `Stated`.
    let origin = match MemoryProvenance::try_from(entry.provenance) {
        Ok(MemoryProvenance::Stated) => MemoryOrigin::Stated,
        Ok(MemoryProvenance::Inferred) => MemoryOrigin::Inferred,
        Ok(MemoryProvenance::Unspecified) | Err(_) => MemoryOrigin::Unrecorded,
    };

    // The topic is the most identifying thing the record carries, and an
    // unrecognised topic is still a better label than a generic one — which is
    // exactly why there is no allowlist of known topics here.
    let topic = entry.topic.trim();
    let label = if topic.is_empty() {
        UNLABELLED.to_owned()
    } else {
        topic.to_owned()
    };

    RecalledMemoryView {
        memory_id: entry.memory_id.clone(),
        role,
        origin,
        label,
        preview: preview_of(&entry.content),
    }
}

/// Bounded, boundary-safe excerpt with an explicit truncation marker.
fn preview_of(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.chars().count() <= MEMORY_PREVIEW_CHARS {
        return trimmed.to_owned();
    }
    // `chars().take()` cannot split a multi-byte character.
    let head: String = trimmed.chars().take(MEMORY_PREVIEW_CHARS).collect();
    format!("{head}{TRUNCATION_SUFFIX}")
}

/// Project the entries a turn recalled, bounded by [`MAX_DESCRIBED_MEMORIES`].
///
/// Entries with no readable content are dropped: a row a reader cannot read
/// tells them nothing and still costs a line.
#[must_use]
pub fn describe_recalled_memories(entries: &[MemoryEntry]) -> Vec<RecalledMemoryView> {
    entries
        .iter()
        .filter(|entry| !entry.content.trim().is_empty())
        .take(MAX_DESCRIBED_MEMORIES)
        .map(project_recalled_memory)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(overrides: impl FnOnce(&mut MemoryEntry)) -> MemoryEntry {
        let mut entry = MemoryEntry {
            memory_id: "mem-1".to_owned(),
            thread_id: "thread-a".to_owned(),
            topic: "USER".to_owned(),
            content: "Prefers metric units".to_owned(),
            score: 0.9,
            updated_at: None,
            user_id: "user-1".to_owned(),
            provenance: MemoryProvenance::Stated as i32,
        };
        overrides(&mut entry);
        entry
    }

    /// The honesty rule the proto states in its own comment: a pre-provenance
    /// row must not be presented as something the user said.
    #[test]
    fn unspecified_provenance_is_never_rendered_as_stated() {
        let view = project_recalled_memory(&entry(|e| {
            e.provenance = MemoryProvenance::Unspecified as i32
        }));
        assert_eq!(view.origin, MemoryOrigin::Unrecorded);
        assert_ne!(view.origin, MemoryOrigin::Stated);
    }

    /// A provenance value added to the proto after this build shipped must also
    /// land on `Unrecorded` — the no-allowlist property. Reading an unknown
    /// value as `Stated` would manufacture consent the record does not support.
    #[test]
    fn an_unknown_provenance_value_degrades_to_unrecorded() {
        let view = project_recalled_memory(&entry(|e| e.provenance = 9_999));
        assert_eq!(view.origin, MemoryOrigin::Unrecorded);
    }

    #[test]
    fn stated_and_inferred_are_distinguished() {
        assert_eq!(
            project_recalled_memory(&entry(|_| {})).origin,
            MemoryOrigin::Stated
        );
        assert_eq!(
            project_recalled_memory(&entry(|e| e.provenance = MemoryProvenance::Inferred as i32))
                .origin,
            MemoryOrigin::Inferred
        );
    }

    /// The role split: another conversation's material reads differently from an
    /// org fact, and a reader needs to tell them apart.
    #[test]
    fn an_entry_with_no_owning_thread_is_org_context_not_recall() {
        let view = project_recalled_memory(&entry(|e| e.thread_id = String::new()));
        assert_eq!(view.role, MemoryRole::Inject);

        let view = project_recalled_memory(&entry(|e| e.thread_id = "thread-b".into()));
        assert_eq!(view.role, MemoryRole::Recall);
    }

    /// No allowlist of topics: an unrecognised one is still a better label than
    /// a generic fallback, because it came from the record.
    #[test]
    fn an_unrecognised_topic_is_used_as_the_label() {
        let view = project_recalled_memory(&entry(|e| e.topic = "SOME_FUTURE_TOPIC".into()));
        assert_eq!(view.label, "SOME_FUTURE_TOPIC");
    }

    #[test]
    fn a_missing_topic_falls_back_to_a_name_not_an_id() {
        let view = project_recalled_memory(&entry(|e| e.topic = "  ".into()));
        assert_eq!(view.label, UNLABELLED);
        assert!(
            !view.label.contains("mem-1"),
            "an id is not a name and tells a reader nothing"
        );
    }

    #[test]
    fn a_long_preview_is_truncated_on_a_character_boundary_and_marked() {
        let long = "æ".repeat(MEMORY_PREVIEW_CHARS * 2);
        let view = project_recalled_memory(&entry(|e| e.content = long));
        assert!(view.preview.ends_with(TRUNCATION_SUFFIX));
        assert_eq!(
            view.preview.chars().count(),
            MEMORY_PREVIEW_CHARS + 1,
            "the marker is the only character past the budget"
        );
        assert!(!view.preview.contains('\u{fffd}'));
    }

    #[test]
    fn a_short_preview_is_untouched() {
        let view = project_recalled_memory(&entry(|_| {}));
        assert_eq!(view.preview, "Prefers metric units");
        assert!(!view.preview.ends_with(TRUNCATION_SUFFIX));
    }

    #[test]
    fn unreadable_entries_are_dropped_and_the_list_is_bounded() {
        let mut entries: Vec<MemoryEntry> = (0..MAX_DESCRIBED_MEMORIES + 5)
            .map(|i| entry(|e| e.memory_id = format!("mem-{i}")))
            .collect();
        entries.push(entry(|e| e.content = "   ".into()));

        let described = describe_recalled_memories(&entries);
        assert_eq!(described.len(), MAX_DESCRIBED_MEMORIES);
        assert!(
            described.iter().all(|view| !view.preview.is_empty()),
            "a row a reader cannot read costs a line and says nothing"
        );
    }

    #[test]
    fn no_entries_describe_nothing() {
        assert!(describe_recalled_memories(&[]).is_empty());
    }
}
