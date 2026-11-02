//! The revision contract of a Space message (build plan WS12-S4).
//!
//! A message is revised by an append-only row in Session Core's `message_revisions`; every read
//! folds the latest allowed revision into the message. This module holds what the planes share:
//! the revision-state tokens, the Control decision names, the model-visible rendering of an
//! edited or deleted message, and the content-free constants a golden can pin.
//!
//! # `model.thread.revise` payload digest (Control must produce the same bytes)
//!
//! SHA-256 over the header `model.thread.revise\0v1\0`, then each field as
//! `name \0 uint64be(len(value)) value` in this order: `org_id`, `user_id` (the CALLER),
//! `thread_id`, `space_id`, `space_decision_ref`, `recipient_audience_ref`,
//! `recipient_audience_hash`, `privacy_policy_ref`, `resource_authorization_ref`,
//! `content_digest` (`sha256:` hex of the new text; of the empty string for a delete),
//! `action_schema_hash`, `idempotency_key`, `message_id`, `revise_action` (`edit` or `delete`);
//! then `authority_revision\0` + uint64be and `recipient_audience_revision\0` + uint64be.
//! The result is `sha256:` + lowercase hex. It is the thread-append v1 encoding with the two
//! message fields added after `idempotency_key`.

/// `SessionMessage.revision_state` of a message that was edited.
pub const STATE_EDITED: &str = "edited";
/// `SessionMessage.revision_state` of a message that was deleted.
pub const STATE_DELETED: &str = "deleted";

/// `ReviseMessageRequest.action` values.
pub const ACTION_EDIT: &str = "edit";
pub const ACTION_DELETE: &str = "delete";

/// Control decision identity (audience `model-plane`).
pub const DECISION_ACTION_ID: &str = "model.thread.revise";
pub const DECISION_SCHEMA: &str = "sha256:thread-revise-v1";
/// Permission every revise decision carries.
pub const PERMISSION_REVISE: &str = "thread:revise";
/// Permission that lets a manager or owner delete another member's message.
pub const PERMISSION_MODERATE: &str = "thread:moderate";
/// Exactly one of these two rides every decision: the org's original-text retention rule.
pub const PERMISSION_PURGE_ORIGINAL: &str = "revise:purge_original";
pub const PERMISSION_RETAIN_ORIGINAL: &str = "revise:retain_original";

/// Longest edited text, in bytes (the same bound as a member post).
pub const MAX_EDIT_BYTES: usize = 32_768;
/// Most revisions one message may take.
pub const MAX_REVISIONS_PER_MESSAGE: i32 = 50;

/// What the model reads in place of a deleted message.
pub const DELETED_MODEL_LINE: &str = "A message here was deleted by a participant; its content is no longer available and earlier answers to it are not retracted.";
/// Appended to an edited message the model reads.
pub const EDITED_MODEL_MARKER: &str = "[This message was edited after it was sent.]";

/// The closed set of moderation reasons a moderator delete may carry.
pub const MODERATION_REASONS: [&str; 5] =
    ["policy_violation", "spam", "harassment", "privacy", "other"];

/// How a message reaches the model: the role and text to put in its history, or `None` to leave
/// it out. A deleted message becomes one static system line (the agent learns of the deletion on
/// its next turn and nothing is appended to the stored thread); an edited message carries its
/// effective text plus a marker; an unrevised one is unchanged, and an empty unrevised message
/// is still left out. `content` is the already-folded effective content.
#[must_use]
pub fn render_for_model(role: &str, content: &str, state: &str) -> Option<(String, String)> {
    match state {
        STATE_DELETED => Some(("system".to_owned(), DELETED_MODEL_LINE.to_owned())),
        STATE_EDITED if !content.is_empty() => {
            Some((role.to_owned(), format!("{content}\n{EDITED_MODEL_MARKER}")))
        }
        _ if content.is_empty() => None,
        _ => Some((role.to_owned(), content.to_owned())),
    }
}
