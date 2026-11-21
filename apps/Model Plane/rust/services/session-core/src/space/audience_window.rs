//! The audience window of a Space read (WS04-S11): the one helper that turns a verified Control
//! `model.thread.read` decision into the `(ceiling, floor)` pair every transcript read filters on.
//!
//! - The ceiling is the recipient-audience revision the decision was issued under; rows stamped
//!   with a NEWER revision are hidden (authority, always on).
//! - The floor is the optional `history_floor_revision` Control signs for a `since_join` room;
//!   rows stamped with an OLDER revision are hidden. It is a presentation rule, not authority: it
//!   only narrows what a verified member sees, so an unset or off switch shows full history
//!   (fail-open on a presentation rule, never on authority). The switch is a rollout control,
//!   never an authorization input.

use std::sync::atomic::{AtomicBool, Ordering};

use tracing::warn;

use crate::grpc::VerifiedSpaceRead;

/// Session Core switch: honour the signed `history_floor_revision` claim. ON only for the literal
/// `true`, so an unset, empty or mistyped value shows full history.
pub(crate) const HISTORY_FLOOR_ENV: &str = "SESSION_CORE_SPACE_HISTORY_FLOOR";

/// Pure form of [`history_floor_enabled`]: `value` is the raw variable, `None` when unset.
pub(crate) fn history_floor_enabled_from(value: Option<&str>) -> bool {
    mp_contracts::agentic_subscription::switch_value_is_on(value)
}

/// Whether this process applies the history floor.
pub(crate) fn history_floor_enabled() -> bool {
    history_floor_enabled_from(std::env::var(HISTORY_FLOOR_ENV).ok().as_deref())
}

/// The floor a signed claim yields: `None` when the claim is absent (0), does not fit a BIGINT, or
/// the switch is off. A non-zero claim that the off switch drops logs one warning per process, so
/// a rollout that signs floors while the switch is off can be seen; the claim is still ignored.
pub(crate) fn history_floor_from_claim(claim: u64, enabled: bool) -> Option<i64> {
    if !enabled {
        // Warned once per process, as `decision_keys::parse_public_key` warns: the claim is read
        // on every request, so the warning must not repeat.
        static IGNORED_CLAIM_WARNED: AtomicBool = AtomicBool::new(false);
        if claim != 0 && !IGNORED_CLAIM_WARNED.swap(true, Ordering::Relaxed) {
            warn!(
                "a Space read decision signs a history floor, but the \
                 SESSION_CORE_SPACE_HISTORY_FLOOR switch is not the literal true; the floor is \
                 ignored and full history is shown"
            );
        }
        return None;
    }
    if claim == 0 {
        return None;
    }
    i64::try_from(claim).ok()
}

/// The two revision bounds one read applies to message rows; `None` means unbounded on that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct AudienceWindow {
    pub(crate) ceiling: Option<i64>,
    pub(crate) floor: Option<i64>,
}

impl AudienceWindow {
    pub(crate) fn new(ceiling: Option<i64>, floor: Option<i64>) -> Self {
        Self { ceiling, floor }
    }

    /// A window with no history floor (owner reads and callers that predate the floor).
    #[cfg(test)]
    pub(crate) fn ceiling_only(ceiling: Option<i64>) -> Self {
        Self { ceiling, floor: None }
    }
}

/// `audience_window(read)`: the window of a verified Space read, or the unbounded window for an
/// owner-bound read.
pub(crate) fn audience_window(read: Option<VerifiedSpaceRead>) -> AudienceWindow {
    match read {
        Some(read) => AudienceWindow::new(Some(read.recipient_audience_revision), read.history_floor),
        None => AudienceWindow::default(),
    }
}

/// The message predicate of a thread-level floor gate (V3e-sws-2), on the message alias `message`.
///
/// Under a `since_join` floor a thread is listed, named or counted only for a message the reader may
/// see from the floor on: on the active path, not deleted, at or below the ceiling and at or after the
/// floor. An unstamped message passes the ceiling and the floor, as in every message read. With no
/// floor (`floor` is `NULL`) the predicate is TRUE for every message, so each gate keeps the answer it
/// gives without a floor. `ceiling` and `floor` are the placeholders that carry the two bounds.
pub(crate) fn floor_gate_message_sql(message: &str, ceiling: &str, floor: &str) -> String {
    format!(
        "({floor}::BIGINT IS NULL OR ({message}.on_active_path
            AND ({message}.recipient_audience_revision IS NULL
                 OR {message}.recipient_audience_revision <= {ceiling}::BIGINT)
            AND ({message}.recipient_audience_revision IS NULL
                 OR {message}.recipient_audience_revision >= {floor}::BIGINT)
            AND NOT EXISTS (SELECT 1 FROM message_revisions floor_gate_rev
                             WHERE floor_gate_rev.message_id = {message}.id
                               AND floor_gate_rev.action = 'deleted')))"
    )
}

/// The thread-level floor gate of a Space run read (R-sws-01): the thread `thread` (an alias of a `threads`
/// row) holds a message the reader may see from the floor on, as [`floor_gate_message_sql`] states it. A
/// caller appends it only while a floor applies, so a statement with no floor keeps its text. The active
/// path is stated in the EXISTS as well (V3e-sws-1-rel1): the same predicate the gate already carries under
/// a floor, written where the `message_tree` guard can read it.
pub(crate) fn floor_gate_thread_sql(thread: &str, ceiling: &str, floor: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM messages em WHERE em.thread_id = {thread}.id AND em.on_active_path AND {gate})",
        gate = floor_gate_message_sql("em", ceiling, floor),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one switch where ON is the restrictive direction, pinned at this call site. Only the
    /// literal `true` turns the floor on; every other value, unset included, shows full history.
    #[test]
    fn history_floor_is_on_only_for_the_exact_literal_true() {
        assert!(
            history_floor_enabled_from(Some("true")),
            "the literal true is on"
        );
        for off in [
            "", " ", " true ", "true\n", "TRUE", "True", "1", "0", "yes", "on", "enabled", "false",
            "off", "no",
        ] {
            assert!(
                !history_floor_enabled_from(Some(off)),
                "{off:?} must leave the floor off, so full history shows"
            );
        }
        assert!(
            !history_floor_enabled_from(None),
            "an unset variable leaves the floor off"
        );
    }

    /// With the switch on, a signed floor narrows the read to rows from the claimed revision on,
    /// and the ceiling Control signed is kept. An absent claim (0) is no floor.
    #[test]
    fn a_signed_floor_is_honoured_when_the_switch_is_on() {
        let on = history_floor_enabled_from(Some("true"));
        let read = VerifiedSpaceRead {
            recipient_audience_revision: 9,
            history_floor: history_floor_from_claim(7, on),
        };
        assert_eq!(
            audience_window(Some(read)),
            AudienceWindow::new(Some(9), Some(7))
        );
        assert_eq!(history_floor_from_claim(0, on), None);
    }

    /// With the switch off the same signed claim is ignored: full history, as before the claim.
    #[test]
    fn a_signed_floor_is_ignored_while_the_switch_is_off() {
        let off = history_floor_enabled_from(Some("1"));
        let read = VerifiedSpaceRead {
            recipient_audience_revision: 9,
            history_floor: history_floor_from_claim(7, off),
        };
        assert_eq!(
            audience_window(Some(read)),
            AudienceWindow::new(Some(9), None)
        );
    }

    /// V3e-sws-2: the floor gate's message predicate applies the ceiling, the active path and the
    /// deletion check under a floor, and is TRUE for every message with no floor, so each gate keeps
    /// the full-history answer it gives without one.
    #[test]
    fn the_floor_gate_message_predicate_applies_ceiling_path_and_deletion_only_under_a_floor() {
        let sql = floor_gate_message_sql("em", "$3", "$6");
        assert!(
            sql.starts_with("($6::BIGINT IS NULL OR ("),
            "the no-floor branch comes first: {sql}"
        );
        for part in [
            "em.on_active_path",
            "em.recipient_audience_revision <= $3::BIGINT",
            "em.recipient_audience_revision >= $6::BIGINT",
            "floor_gate_rev.action = 'deleted'",
            "floor_gate_rev.message_id = em.id",
        ] {
            assert!(sql.contains(part), "the floor gate states {part}: {sql}");
        }
        let placeholders: Vec<&str> = sql
            .split(|c: char| c == '(' || c == ')' || c.is_whitespace())
            .filter(|token| token.starts_with('$'))
            .collect();
        assert!(
            placeholders
                .iter()
                .all(|token| *token == "$3::BIGINT" || *token == "$6::BIGINT"),
            "only the ceiling and the floor are bound: {placeholders:?}"
        );
    }

    /// R-sws-01: the thread gate is an EXISTS over the thread's own messages that states the message
    /// predicate, and it binds only the two bounds it is given.
    #[test]
    fn the_floor_gate_thread_predicate_states_an_exists_over_the_thread_messages() {
        let sql = floor_gate_thread_sql("t", "$7", "$8");
        assert!(
            sql.starts_with("EXISTS (SELECT 1 FROM messages em WHERE em.thread_id = t.id AND em.on_active_path AND ($8::BIGINT IS NULL OR"),
            "the thread gate reads the thread's own messages, no-floor branch first: {sql}"
        );
        assert!(
            sql.contains(&floor_gate_message_sql("em", "$7", "$8")),
            "the thread gate states the message predicate: {sql}"
        );
        let placeholders: Vec<&str> = sql
            .split(|c: char| c == '(' || c == ')' || c.is_whitespace())
            .filter(|token| token.starts_with('$'))
            .collect();
        assert!(
            placeholders
                .iter()
                .all(|token| *token == "$7::BIGINT" || *token == "$8::BIGINT"),
            "only the ceiling and the floor are bound: {placeholders:?}"
        );
    }
}
