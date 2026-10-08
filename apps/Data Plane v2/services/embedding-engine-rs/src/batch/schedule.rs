//! When a unit is looked at again.
//!
//! An outcome that is not final says when it is due (a throttle says how long the
//! provider asked to wait; an unresolved posture waits five minutes; a zero-retention
//! organization is rechecked hourly; a unit whose inline time ran out is due at once).
//! One kind says nothing: a transient failure with no wait named. For it the unit's own
//! history decides, on a ladder that backs off from a minute to half a day, so that a
//! provider that is down for an afternoon is asked a handful of times and not every
//! minute.
//!
//! The ladder lives here, with the pipeline that writes the first `context_next_attempt_at`,
//! because the reconciler (CR-T9a) must read and extend the same schedule, not keep a
//! second one that agrees until somebody changes a number.

use std::time::Duration;

use crate::context::ContextOutcome;

/// Backoff after the first, second, ... failed attempt of a unit. The last step repeats.
pub(crate) const LADDER: [Duration; 5] = [
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(30 * 60),
    Duration::from_secs(2 * 60 * 60),
    Duration::from_secs(12 * 60 * 60),
];

/// The wait after attempt number `attempt` (1-based) has failed.
pub(crate) fn ladder_step(attempt: u32) -> Duration {
    let index = usize::try_from(attempt.saturating_sub(1)).unwrap_or(usize::MAX);
    LADDER[index.min(LADDER.len() - 1)]
}

/// How long from now until the unit is due again after `outcome`, given how many
/// attempts it had before this one. `None`: nothing is scheduled (the outcome is
/// final, or nobody should look again until something changes).
pub(crate) fn next_due(outcome: &ContextOutcome, attempts_before: i32) -> Option<Duration> {
    match outcome {
        // The provider named no wait: the unit's own history decides.
        ContextOutcome::Transient {
            retry_after: None, ..
        } => Some(ladder_step(
            u32::try_from(attempts_before.max(0))
                .unwrap_or(u32::MAX)
                .saturating_add(1),
        )),
        other => other.retry_after(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ErrorClass, PendingReason, ZdrReason};

    #[test]
    fn the_ladder_backs_off_from_a_minute_to_half_a_day_and_stays_there() {
        let steps: Vec<u64> = (1..=7)
            .map(|attempt| ladder_step(attempt).as_secs())
            .collect();
        assert_eq!(steps, [60, 300, 1_800, 7_200, 43_200, 43_200, 43_200]);
        assert_eq!(
            ladder_step(0),
            ladder_step(1),
            "attempt zero is not before the first"
        );
        assert_eq!(ladder_step(u32::MAX), Duration::from_secs(43_200));
    }

    #[test]
    fn a_transient_failure_with_no_wait_named_follows_the_ladder_by_the_units_history() {
        let outcome = ContextOutcome::transient(ErrorClass::ProviderUnavailable, None);
        assert_eq!(next_due(&outcome, 0), Some(Duration::from_secs(60)));
        assert_eq!(next_due(&outcome, 1), Some(Duration::from_secs(300)));
        assert_eq!(next_due(&outcome, 9), Some(Duration::from_secs(43_200)));
        assert_eq!(
            next_due(&outcome, -3),
            Some(Duration::from_secs(60)),
            "a negative count is no history"
        );
    }

    #[test]
    fn what_the_provider_asked_for_wins_over_the_ladder() {
        let outcome =
            ContextOutcome::transient(ErrorClass::RateLimited, Some(Duration::from_millis(1_500)));
        assert_eq!(next_due(&outcome, 4), Some(Duration::from_millis(1_500)));
        // A wait longer than the cap is capped by the outcome itself (five minutes).
        let outcome =
            ContextOutcome::transient(ErrorClass::RateLimited, Some(Duration::from_secs(52_260)));
        assert_eq!(next_due(&outcome, 0), Some(Duration::from_secs(300)));
    }

    #[test]
    fn the_waits_the_outcomes_carry_are_the_ones_scheduled() {
        let pending =
            ContextOutcome::pending(PendingReason::PostureUnavailable, Duration::from_secs(300));
        assert_eq!(next_due(&pending, 0), Some(Duration::from_secs(300)));
        let deadline = ContextOutcome::pending(PendingReason::DeadlineExceeded, Duration::ZERO);
        assert_eq!(next_due(&deadline, 0), Some(Duration::ZERO), "due at once");
        let org = ContextOutcome::skipped_zdr(ZdrReason::Org);
        assert_eq!(
            next_due(&org, 0),
            Some(Duration::from_secs(3_600)),
            "rechecked hourly"
        );
        assert_eq!(
            next_due(&ContextOutcome::skipped_zdr(ZdrReason::Document), 0),
            None,
            "a restricted document is final"
        );
        assert_eq!(
            next_due(&ContextOutcome::permanent(ErrorClass::ContentFilter), 0),
            None
        );
        assert_eq!(next_due(&ContextOutcome::SkippedPolicy, 0), None);
        // The budget gate (CR-T9b): a stop is final until the gate lets the organization through
        // (it is read now, not scheduled), and a throttled chunk is due at once, because the
        // reconciler's pace is the throttle.
        assert_eq!(next_due(&ContextOutcome::SkippedBudget, 0), None);
        let throttled = ContextOutcome::pending(PendingReason::BudgetThrottled, Duration::ZERO);
        assert_eq!(next_due(&throttled, 0), Some(Duration::ZERO));
    }
}
