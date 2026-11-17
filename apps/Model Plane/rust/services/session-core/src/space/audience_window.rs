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
/// the switch is off.
pub(crate) fn history_floor_from_claim(claim: u64, enabled: bool) -> Option<i64> {
    if !enabled || claim == 0 {
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
}
