//! Recovering conversation the prompt no longer carries.
//!
//! # The dead end this replaces
//!
//! When a prompt outgrows the model's input limit, `compaction` either
//! summarizes the head or drops it, and leaves a notice behind. The notice was
//! honest but terminal: *"If the user refers to something you cannot see here,
//! say so instead of guessing."* The model's only move was to apologise.
//!
//! The messages were never lost. Compaction edits the **prompt**, not the
//! durable thread — session-core still has every turn. So "I cannot see that"
//! was only ever true of the request, and the fix is to let the model go and
//! read what it needs.
//!
//! # Why a tool and not automatic re-attachment
//!
//! Re-attaching eagerly would undo the compaction that made the turn fit at all.
//! Only the model knows whether *this* question depends on something the prompt
//! is missing, so it asks — the same reasoning that makes
//! `CLEARED_TOOL_RESULT_NOTICE` tell the model to call the tool again rather
//! than re-running every cleared tool for it.
//!
//! # Bounded on purpose
//!
//! [`MAX_REATTACH_CHARS`] caps one recovery. Without a ceiling the model could
//! pull the whole history back in one call and re-trigger the overflow that
//! caused the compaction, which would look like a loop rather than a recovery.
//! An over-budget recovery is truncated **with the truncation stated**, because
//! silently returning half a conversation is how a model concludes something
//! confidently from evidence it only partly received.

use std::fmt::Write as _;

use mp_contracts::model_plane::v1::SessionMessage;

/// Most characters one reattachment may return.
pub const MAX_REATTACH_CHARS: usize = 6_000;

/// Most messages one reattachment may return, independent of the char budget —
/// forty two-word turns are as unhelpful as one enormous one.
pub const MAX_REATTACH_MESSAGES: usize = 20;

/// Stated when the recovery hit its ceiling.
pub const REATTACH_TRUNCATED_NOTICE: &str =
    "[This recovery hit its size limit. Earlier messages than the ones above were \
     not returned — narrow your query and ask again rather than assuming this is \
     the whole history.]";

/// Returned when the thread genuinely has nothing matching.
pub const REATTACH_NO_MATCH_NOTICE: &str =
    "[No earlier message in this conversation matches that. Say so plainly rather \
     than guessing at what it might have been.]";

/// One recovered message, flattened for the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredMessage {
    /// 1-based position in the durable thread, so the model can refer to it and
    /// the reader can find it.
    pub position: usize,
    pub role: String,
    pub content: String,
}

/// The outcome of one recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reattachment {
    pub messages: Vec<RecoveredMessage>,
    /// True when the ceiling cut the result short.
    pub truncated: bool,
    /// Total messages that matched before the ceiling applied, so the model can
    /// tell "there were only two" from "there were two hundred".
    pub matched: usize,
}

/// Normalize the model's query. An empty query means "the earliest history",
/// which is the common case after a head-drop.
fn normalize(query: &str) -> String {
    query.trim().to_lowercase()
}

/// Select the messages a recovery should return.
///
/// `already_in_prompt` is the tail the request still carries. Those are excluded:
/// returning a message the model can already see spends the budget on nothing and
/// invites it to treat one turn as two separate pieces of evidence.
#[must_use]
pub fn select_reattachment(
    thread: &[SessionMessage],
    query: &str,
    already_in_prompt: &[String],
) -> Reattachment {
    let needle = normalize(query);
    let present: Vec<String> = already_in_prompt
        .iter()
        .map(|content| content.trim().to_lowercase())
        .filter(|content| !content.is_empty())
        .collect();

    let candidates: Vec<RecoveredMessage> = thread
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            matches!(message.role.as_str(), "user" | "assistant")
                && !message.content.trim().is_empty()
        })
        .filter(|(_, message)| {
            let content = message.content.trim().to_lowercase();
            !present.iter().any(|seen| seen == &content)
        })
        .filter(|(_, message)| {
            // An empty query recovers the oldest history wholesale; otherwise
            // match on content. Substring rather than fuzzy: a recovery that
            // returns loosely-related turns is worse than one that returns
            // nothing, because the model cannot tell which it got.
            needle.is_empty() || message.content.to_lowercase().contains(&needle)
        })
        .map(|(index, message)| RecoveredMessage {
            position: index + 1,
            role: message.role.clone(),
            content: message.content.trim().to_owned(),
        })
        .collect();

    let matched = candidates.len();
    let mut messages = Vec::new();
    let mut used = 0usize;
    let mut truncated = false;
    for candidate in candidates {
        if messages.len() >= MAX_REATTACH_MESSAGES {
            truncated = true;
            break;
        }
        let cost = candidate.content.chars().count();
        if used + cost > MAX_REATTACH_CHARS {
            truncated = true;
            break;
        }
        used += cost;
        messages.push(candidate);
    }

    Reattachment {
        messages,
        truncated,
        matched,
    }
}

/// Render a recovery as the tool's output text.
#[must_use]
pub fn render_reattachment(result: &Reattachment) -> String {
    if result.messages.is_empty() {
        return REATTACH_NO_MATCH_NOTICE.to_owned();
    }
    let mut out = String::new();
    for message in &result.messages {
        let _ = writeln!(
            out,
            "#{} {}: {}",
            message.position, message.role, message.content
        );
    }
    if result.truncated {
        out.push_str(REATTACH_TRUNCATED_NOTICE);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: &str, content: &str) -> SessionMessage {
        SessionMessage {
            message_id: String::new(),
            role: role.to_owned(),
            content: content.to_owned(),
            ..Default::default()
        }
    }

    fn thread() -> Vec<SessionMessage> {
        vec![
            message("user", "Vi bestemte at fakturaer godkjennes av økonomi"),
            message("assistant", "Notert — økonomi godkjenner fakturaer"),
            message("user", "Hva er været i Bergen?"),
            message("assistant", "Det regner"),
        ]
    }

    #[test]
    fn a_query_recovers_only_matching_history() {
        let result = select_reattachment(&thread(), "fakturaer", &[]);
        assert_eq!(result.matched, 2);
        assert!(result
            .messages
            .iter()
            .all(|m| m.content.contains("faktura")));
        assert!(!result.truncated);
    }

    /// Positions are 1-based and point into the DURABLE thread, so the model can
    /// refer to a recovered message and a reader can find it.
    #[test]
    fn positions_point_into_the_durable_thread() {
        let result = select_reattachment(&thread(), "været", &[]);
        assert_eq!(result.messages[0].position, 3);
    }

    /// Returning something the prompt already carries spends the budget on
    /// nothing and invites the model to double-count one turn as two.
    #[test]
    fn messages_already_in_the_prompt_are_excluded() {
        let present = vec!["Det regner".to_owned()];
        let result = select_reattachment(&thread(), "regner", &present);
        assert_eq!(result.matched, 0);
        assert!(result.messages.is_empty());
    }

    #[test]
    fn an_empty_query_recovers_the_oldest_history() {
        let result = select_reattachment(&thread(), "   ", &[]);
        assert_eq!(result.matched, 4);
        assert_eq!(result.messages[0].position, 1);
    }

    /// The ceiling exists so a recovery cannot re-trigger the overflow that
    /// caused the compaction. Hitting it must be STATED.
    #[test]
    fn an_over_budget_recovery_is_truncated_and_says_so() {
        let long: Vec<SessionMessage> = (0..40)
            .map(|i| message("user", &format!("{} {}", i, "x".repeat(400))))
            .collect();
        let result = select_reattachment(&long, "", &[]);
        assert!(result.truncated);
        assert!(result.matched > result.messages.len());

        let rendered = render_reattachment(&result);
        assert!(rendered.contains(REATTACH_TRUNCATED_NOTICE));
        assert!(
            result
                .messages
                .iter()
                .map(|m| m.content.chars().count())
                .sum::<usize>()
                <= MAX_REATTACH_CHARS
        );
    }

    #[test]
    fn the_message_ceiling_applies_independently_of_the_char_budget() {
        // Short messages: the char budget would never trigger, so only the
        // message ceiling can stop this.
        let many: Vec<SessionMessage> = (0..MAX_REATTACH_MESSAGES + 10)
            .map(|i| message("user", &format!("kort {i}")))
            .collect();
        let result = select_reattachment(&many, "kort", &[]);
        assert_eq!(result.messages.len(), MAX_REATTACH_MESSAGES);
        assert!(result.truncated);
    }

    /// No match is a real outcome and must be said, not returned as emptiness
    /// the model can read as "nothing was ever discussed".
    #[test]
    fn no_match_is_stated_explicitly() {
        let result = select_reattachment(&thread(), "kvantefysikk", &[]);
        assert_eq!(result.matched, 0);
        assert_eq!(render_reattachment(&result), REATTACH_NO_MATCH_NOTICE);
    }

    #[test]
    fn system_messages_are_never_recovered() {
        let mut thread = thread();
        thread.insert(0, message("system", "Du er Verevon"));
        let result = select_reattachment(&thread, "Verevon", &[]);
        assert_eq!(result.matched, 0, "the system prompt is not conversation");
    }

    #[test]
    fn a_recovery_renders_role_and_position_for_each_message() {
        let rendered = render_reattachment(&select_reattachment(&thread(), "fakturaer", &[]));
        assert!(rendered.contains("#1 user:"));
        assert!(rendered.contains("#2 assistant:"));
    }
}
