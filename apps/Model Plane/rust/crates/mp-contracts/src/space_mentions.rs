//! Identity-bound mentions and addressed agent wake-up (build plan WS08-S2).
//!
//! A human post can carry `mentions` (ids, never text). An agent mention is either `expected`
//! (a wake may start one run for it) or `refused` (recorded, no run may start). A wake starts one
//! run from a stored post and carries a Control-signed `model.run.wake` decision; the signed
//! `turn_origin` is stored on `runs`. The two switches below are rollout controls, never
//! authorization inputs: Control's signed decision and Session Core's wake verifier decide.
//!
//! # `model.run.wake` payload digest (Control's `runWakePayloadDigest` produces the same bytes)
//!
//! SHA-256 over the header `model.run.wake\0v1\0`, then each field as
//! `name \0 uint64be(len(value)) value` in this order: `org_id`, `user_id` (the CALLER),
//! `thread_id`, `space_id`, `space_decision_ref`, `recipient_audience_ref`,
//! `recipient_audience_hash`, `privacy_policy_ref`, `resource_authorization_ref`,
//! `trigger_message_id`, `content_digest` (`sha256:` hex of the stored post's text),
//! `agent_subject_id`, `turn_origin`, `action_schema_hash`, `idempotency_key`; then
//! `authority_revision\0` + uint64be and `recipient_audience_revision\0` + uint64be. The result is
//! `sha256:` + lowercase hex. The shared vector is `tests/fixtures/run-wake-v1.golden.json`.

use crate::agentic_subscription::switch_value_is_on;

/// Session Core: records mentions beside a post.
pub const RECORDS_CORE_ENV: &str = "SPACE_MENTION_RECORDS_CORE";
/// Session Core: accepts a wake on `StartManagedRun`.
pub const WAKE_CORE_ENV: &str = "SPACE_WAKE_CORE";
/// model-gateway: accepts `mentions` on the post routes and a `wake_context` on the invoke stream
/// (WS08-S3).
pub const WAKE_API_ENV: &str = "SPACE_WAKE_API";
/// model-gateway: lets a wake read the newest room posts as fenced, quoted context (WS08-S3,
/// default off until decision D-6).
pub const WAKE_ROOM_CONTEXT_ENV: &str = "SPACE_WAKE_ROOM_CONTEXT";
/// The most room posts a wake may read as fenced context.
pub const ROOM_CONTEXT_POSTS: u32 = 7;

/// Pure form of the switches: `value` is the raw variable, `None` when unset.
#[must_use]
pub fn enabled_from(value: Option<&str>) -> bool {
    switch_value_is_on(value)
}

/// Whether mention rows are recorded by this Session Core process.
#[must_use]
pub fn records_core_enabled() -> bool {
    enabled_from(std::env::var(RECORDS_CORE_ENV).ok().as_deref())
}

/// Whether a wake is accepted by this Session Core process.
#[must_use]
pub fn wake_core_enabled() -> bool {
    enabled_from(std::env::var(WAKE_CORE_ENV).ok().as_deref())
}

/// Whether this model-gateway process accepts `mentions` on posts and a wake on the stream.
#[must_use]
pub fn wake_api_enabled() -> bool {
    enabled_from(std::env::var(WAKE_API_ENV).ok().as_deref())
}

/// Whether a wake may read the newest room posts as fenced context.
#[must_use]
pub fn wake_room_context_enabled() -> bool {
    enabled_from(std::env::var(WAKE_ROOM_CONTEXT_ENV).ok().as_deref())
}

/// Control decision identity (audience `model-plane`).
pub const DECISION_ACTION_ID: &str = "model.run.wake";
pub const DECISION_SCHEMA: &str = "sha256:run-wake-v1";
/// The only permission a wake decision carries; disjoint from the thread permissions.
pub const PERMISSION_WAKE: &str = "run:wake";

/// Most people one post may mention.
pub const MAX_PEOPLE_MENTIONS: usize = 20;
/// Most agents one post may mention (and so most wakes of one post).
pub const MAX_AGENT_MENTIONS: usize = 3;

/// `message_mentions.subject_kind`.
pub const KIND_USER: &str = "user";
pub const KIND_AGENT: &str = "agent";

/// `message_mentions.source`.
pub const SOURCE_TYPED: &str = "typed";
pub const SOURCE_LOCKED: &str = "locked";

/// `message_mentions.wake_expectation`.
pub const WAKE_EXPECTED: &str = "expected";
pub const WAKE_REFUSED: &str = "refused";

/// `message_mentions.refusal_code`.
pub const REFUSAL_RESPOND_TO_DENIED: &str = "respond_to_denied";
pub const REFUSAL_MENTION_NOT_ENABLED: &str = "mention_not_enabled";
pub const REFUSAL_INVOCATION_BLOCKED: &str = "invocation_blocked";
pub const REFUSAL_OWNER_ONLY_THREAD: &str = "owner_only_thread";
/// The closed set of refusal codes.
pub const REFUSAL_CODES: [&str; 4] = [
    REFUSAL_RESPOND_TO_DENIED,
    REFUSAL_MENTION_NOT_ENABLED,
    REFUSAL_INVOCATION_BLOCKED,
    REFUSAL_OWNER_ONLY_THREAD,
];

/// `runs.turn_origin`. NULL on a legacy run reads as `direct`.
pub const ORIGIN_DIRECT: &str = "direct";
pub const ORIGIN_AMBIENT: &str = "ambient";
pub const ORIGIN_AUTOMATION: &str = "automation";

/// Derived wake states a read reports (`MentionRef.wake_state`).
pub const STATE_REFUSED: &str = "refused";
pub const STATE_EXPECTED: &str = "expected";
pub const STATE_RUNNING: &str = "running";
pub const STATE_AWAITING_APPROVAL: &str = "awaiting_approval";
pub const STATE_COMPLETED: &str = "completed";
pub const STATE_FAILED: &str = "failed";
pub const STATE_CANCELLED: &str = "cancelled";

/// The run origin (`runs.metadata.origin`, see [`crate::run_origin`]) a turn origin is recorded
/// as (ADR-0012, X07). The Go twin is `RunOriginForTurnOrigin` in user-core `spaces`.
#[must_use]
pub fn run_origin_for_turn_origin(turn_origin: &str) -> Option<&'static str> {
    match turn_origin {
        ORIGIN_DIRECT => Some("chat"),
        ORIGIN_AMBIENT => Some("trigger"),
        ORIGIN_AUTOMATION => Some("schedule"),
        _ => None,
    }
}

/// Whether `id` is a valid mentioned subject id: `^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`.
#[must_use]
pub fn valid_subject_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id.len() <= 128
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_are_on_only_for_an_on_word() {
        assert!(!enabled_from(None));
        assert!(!enabled_from(Some("")));
        assert!(!enabled_from(Some("0")));
        assert!(enabled_from(Some(" TRUE ")));
    }

    #[test]
    fn subject_id_pattern() {
        assert!(valid_subject_id("user-1"));
        assert!(valid_subject_id("a.b:c_d-e"));
        assert!(!valid_subject_id(""));
        assert!(!valid_subject_id("-lead"));
        assert!(!valid_subject_id("has space"));
        assert!(!valid_subject_id(&"a".repeat(129)));
    }

    #[test]
    fn origin_mapping_matches_the_golden_table() {
        assert_eq!(run_origin_for_turn_origin("direct"), Some("chat"));
        assert_eq!(run_origin_for_turn_origin("ambient"), Some("trigger"));
        assert_eq!(run_origin_for_turn_origin("automation"), Some("schedule"));
        assert_eq!(run_origin_for_turn_origin("api"), None);
    }
}
