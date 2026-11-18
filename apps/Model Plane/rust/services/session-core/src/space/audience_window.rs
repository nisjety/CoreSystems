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

/// Session Core switch: honour the signed `history_floor_revision` claim. ON only for an explicit
/// on-word, so an unset, empty or mistyped value shows full history.
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

    /// The largest claim a signed `history_floor_revision` can carry and still fit a BIGINT.
    const MAX_FITTING_CLAIM: u64 = i64::MAX.unsigned_abs();

    #[test]
    fn a_zero_claim_a_switch_off_and_an_over_bigint_claim_yield_no_floor() {
        // An absent claim (0) is no floor, whatever the switch says.
        assert_eq!(history_floor_from_claim(0, true), None);
        assert_eq!(history_floor_from_claim(0, false), None);
        // Switch off: a real claim is ignored, so the reader sees full history.
        assert_eq!(history_floor_from_claim(1, false), None);
        assert_eq!(history_floor_from_claim(7, false), None);
        assert_eq!(history_floor_from_claim(MAX_FITTING_CLAIM, false), None);
        assert_eq!(history_floor_from_claim(u64::MAX, false), None);
        // A claim that does not fit a BIGINT is dropped, never wrapped into a negative floor.
        assert_eq!(history_floor_from_claim(MAX_FITTING_CLAIM + 1, true), None);
        assert_eq!(history_floor_from_claim(u64::MAX, true), None);
    }

    #[test]
    fn a_claim_that_fits_with_the_switch_on_is_the_floor() {
        assert_eq!(history_floor_from_claim(1, true), Some(1));
        assert_eq!(history_floor_from_claim(7, true), Some(7));
        assert_eq!(history_floor_from_claim(MAX_FITTING_CLAIM, true), Some(i64::MAX));
    }

    #[test]
    fn the_floor_switch_is_on_only_for_an_explicit_on_word() {
        for on in ["1", "true", "on", "yes", "enabled", " TRUE ", "On", "Enabled"] {
            assert!(history_floor_enabled_from(Some(on)), "{on:?} must turn the floor on");
        }
        for off in [
            "", " ", "0", "false", "off", "no", "disabled", "2", "y", "tru", "enable", "yes please", "null",
        ] {
            assert!(!history_floor_enabled_from(Some(off)), "{off:?} must leave the floor off");
        }
        assert!(!history_floor_enabled_from(None), "an unset variable is off");
    }

    #[test]
    fn the_window_is_unbounded_for_an_owner_read_and_bounded_by_a_verified_one() {
        assert_eq!(
            audience_window(None),
            AudienceWindow { ceiling: None, floor: None },
            "an owner-bound read has neither bound"
        );
        let both = audience_window(Some(VerifiedSpaceRead {
            recipient_audience_revision: 7,
            history_floor: Some(3),
        }));
        assert_eq!(both, AudienceWindow::new(Some(7), Some(3)));
        let ceiling_only = audience_window(Some(VerifiedSpaceRead {
            recipient_audience_revision: 7,
            history_floor: None,
        }));
        assert_eq!(ceiling_only, AudienceWindow::ceiling_only(Some(7)));
        assert_eq!(ceiling_only.floor, None, "no floor claim: full history up to the ceiling");
    }
}
