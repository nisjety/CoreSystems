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
//! - The timestamp floor (`floor_at`, D18) is the optional `history_floor_at` Control signs for a
//!   per-member history window (a `since_date` window, or a member's join). Rows created BEFORE
//!   that instant are hidden. Control signs it only for a member's own row, and it is honoured
//!   WITHOUT the switch above: a hide must never depend on a rollout control. It is still a
//!   narrowing only, and a row stays visible only when every floor that is set admits it.

use chrono::{DateTime, Utc};

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

/// The bounds one read applies to message rows: the revision ceiling and the two floors. `None`
/// means unbounded on that side. A row is visible when it is at or below the ceiling, at or above
/// the revision floor, and created at or after the timestamp floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct AudienceWindow {
    pub(crate) ceiling: Option<i64>,
    pub(crate) floor: Option<i64>,
    /// D18: the instant before which no row is visible (a per-member since_date window, or a
    /// member's join). `None` means no timestamp floor.
    pub(crate) floor_at: Option<DateTime<Utc>>,
}

impl AudienceWindow {
    pub(crate) fn new(ceiling: Option<i64>, floor: Option<i64>) -> Self {
        Self {
            ceiling,
            floor,
            floor_at: None,
        }
    }

    /// The same window with a timestamp floor (D18). `None` leaves the window unchanged.
    pub(crate) fn with_floor_at(mut self, floor_at: Option<DateTime<Utc>>) -> Self {
        self.floor_at = floor_at;
        self
    }

    /// Whether this read hides any row by a floor (either form). A read with a floor shows no
    /// presentation fallback that could carry pre-floor content (a stored title or preview).
    /// The attention read (C-PRIO-05) states this rule in SQL on the bound floors, so the only
    /// callers today are the tests that pin it.
    #[allow(dead_code)]
    pub(crate) fn has_floor(&self) -> bool {
        self.floor.is_some() || self.floor_at.is_some()
    }

    /// A window with no history floor (owner reads and callers that predate the floor).
    #[cfg(test)]
    pub(crate) fn ceiling_only(ceiling: Option<i64>) -> Self {
        Self::new(ceiling, None)
    }
}

/// The floors on one message row `m` (WS04-S11 revision floor, D18 timestamp floor), as the predicates the
/// message reads apply. `(floor_rev, floor_at)` are the placeholders the query binds the two floors to. A
/// NULL-stamped row passes the revision floor, and the timestamp floor hides a row created before its instant.
/// The row must be aliased `m`. With no floor bound (both NULL) it admits every row.
pub(crate) fn message_floor_sql((floor_rev, floor_at): (&str, &str)) -> String {
    format!(
        "({floor_rev}::BIGINT IS NULL OR m.recipient_audience_revision IS NULL
                 OR m.recipient_audience_revision >= {floor_rev}::BIGINT)
         AND ({floor_at}::TIMESTAMPTZ IS NULL OR m.created_at >= {floor_at}::TIMESTAMPTZ)"
    )
}

/// D18: whether an instant (a run's start, an approval's request, an event's time) is inside a
/// member's timestamp floor. `true` with no floor. This is the Rust copy of the `>= floor` predicate
/// the SQL applies; reads that hold the instant check it again on the rows they return, so a
/// predicate that drifts in SQL still cannot show a pre-floor row.
pub(crate) fn floor_admits_at(at: DateTime<Utc>, floor_at: Option<DateTime<Utc>>) -> bool {
    floor_at.map_or(true, |floor| at >= floor)
}

/// `audience_window(read)`: the window of a verified Space read, or the unbounded window for an
/// owner-bound read.
pub(crate) fn audience_window(read: Option<VerifiedSpaceRead>) -> AudienceWindow {
    match read {
        Some(read) => {
            AudienceWindow::new(Some(read.recipient_audience_revision), read.history_floor)
                .with_floor_at(read.history_floor_at)
        }
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
            AudienceWindow { ceiling: None, floor: None, floor_at: None },
            "an owner-bound read has no bound"
        );
        let both = audience_window(Some(VerifiedSpaceRead {
            recipient_audience_revision: 7,
            history_floor: Some(3),
            history_floor_at: None,
        }));
        assert_eq!(both, AudienceWindow::new(Some(7), Some(3)));
        let ceiling_only = audience_window(Some(VerifiedSpaceRead {
            recipient_audience_revision: 7,
            history_floor: None,
            history_floor_at: None,
        }));
        assert_eq!(ceiling_only, AudienceWindow::ceiling_only(Some(7)));
        assert_eq!(ceiling_only.floor, None, "no floor claim: full history up to the ceiling");
        assert!(!ceiling_only.has_floor());
    }

    #[test]
    fn a_floor_admits_an_instant_at_or_after_it_and_nothing_before_it() {
        let floor = DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").expect("rfc3339").with_timezone(&Utc);
        let before = floor - chrono::Duration::milliseconds(1);
        let after = floor + chrono::Duration::milliseconds(1);
        assert!(floor_admits_at(before, None), "no floor admits every instant");
        assert!(!floor_admits_at(before, Some(floor)), "a millisecond before the floor is hidden");
        assert!(floor_admits_at(floor, Some(floor)), "the floor instant itself is admitted");
        assert!(floor_admits_at(after, Some(floor)));
    }

    #[test]
    fn a_timestamp_floor_rides_the_window_without_the_revision_switch() {
        // D18: a per-member instant is carried on the window whether or not the revision claim is
        // honoured, so a since_date member is never shown the rows before the date.
        let at = DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").expect("rfc3339").with_timezone(&Utc);
        let windowed = audience_window(Some(VerifiedSpaceRead {
            recipient_audience_revision: 7,
            history_floor: None,
            history_floor_at: Some(at),
        }));
        assert_eq!(windowed.ceiling, Some(7));
        assert_eq!(windowed.floor, None, "no revision floor was signed");
        assert_eq!(windowed.floor_at, Some(at));
        assert!(windowed.has_floor(), "a timestamp floor is a floor for the presentation fallbacks");
        assert_eq!(windowed, AudienceWindow::new(Some(7), None).with_floor_at(Some(at)));
    }
}
