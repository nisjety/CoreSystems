//! The run's reasoning level on each inference round (build plan WS11 S8, DEC-36 Q1).
//!
//! The gateway sets `RunAgentRequest.reasoning_effort` on a fresh Do or Plan run. The
//! executor sends it on every round the run makes, delegated children included, but only
//! when `EXECUTION_CORE_RUN_EFFORT` is the literal `true`. With the switch off, every
//! `InferRequest` is the one it was before the field existed.

use mp_contracts::model_plane::v1 as pb;
use tracing::warn;

/// The switch. Off (the default) keeps every round at Unspecified.
pub(crate) const RUN_EFFORT_ENV: &str = "EXECUTION_CORE_RUN_EFFORT";

/// Whether an explicit value turns the switch on. Pure so the policy is testable without
/// the process environment. Only the literal `true` counts (after trimming), the same
/// strict parser as model-gateway's `parse_ladder`: a stray `1`, `on` or `yes` must not
/// start spending reasoning on a deployed agent.
#[must_use]
pub(crate) fn parse_enabled(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

#[cfg(test)]
thread_local! {
    /// A test's override of the switch. `#[tokio::test]` runs its future on the test's own
    /// thread, so a thread-local cannot leak into a parallel test the way a process-wide
    /// environment variable would.
    static TEST_RUN_EFFORT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Holds the run-effort switch at a fixed value for the current test thread, then restores it.
#[cfg(test)]
pub(crate) struct RunEffortSwitch;

#[cfg(test)]
impl RunEffortSwitch {
    pub(crate) fn set(on: bool) -> Self {
        TEST_RUN_EFFORT.with(|cell| cell.set(Some(on)));
        Self
    }
}

#[cfg(test)]
impl Drop for RunEffortSwitch {
    fn drop(&mut self) {
        TEST_RUN_EFFORT.with(|cell| cell.set(None));
    }
}

/// Whether this process forwards a run's reasoning level to inference-core.
#[must_use]
pub(crate) fn enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = TEST_RUN_EFFORT.with(std::cell::Cell::get) {
        return forced;
    }
    parse_enabled(std::env::var(RUN_EFFORT_ENV).ok().as_deref())
}

/// The `InferRequest.reasoning_effort` one round sends, given the run's own level `wire`.
/// Off, a round sends Unspecified. On, a level this executor does not know also sends
/// Unspecified: it is logged by its number alone, and it never fails the round.
#[must_use]
pub(crate) fn round_level(on: bool, wire: i32) -> i32 {
    if !on {
        return 0;
    }
    match pb::ReasoningEffort::try_from(wire) {
        Ok(level) => level as i32,
        Err(_) => {
            warn!(
                wire,
                "run_agent: unknown reasoning level, the round sends none"
            );
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_effort_switch_accepts_only_the_literal_true() {
        assert!(parse_enabled(Some("true")));
        assert!(parse_enabled(Some(" true\n")));
        for raw in ["1", "on", "yes", "TRUE", "True", ""] {
            assert!(
                !parse_enabled(Some(raw)),
                "{raw:?} must not turn the switch on"
            );
        }
        assert!(!parse_enabled(None));
    }

    #[test]
    fn run_effort_test_switch_overrides_the_environment() {
        let _on = RunEffortSwitch::set(true);
        assert!(enabled());
        let _off = RunEffortSwitch::set(false);
        assert!(!enabled());
    }

    #[test]
    fn run_effort_off_sends_unspecified_whatever_the_run_asked() {
        assert_eq!(round_level(false, pb::ReasoningEffort::High as i32), 0);
        assert_eq!(round_level(false, 99), 0);
    }

    #[test]
    fn run_effort_on_forwards_the_runs_own_level() {
        let high = pb::ReasoningEffort::High as i32;
        assert_eq!(round_level(true, high), high);
        assert_eq!(round_level(true, 0), 0);
    }

    #[test]
    fn run_effort_on_with_an_unknown_level_sends_unspecified() {
        assert_eq!(round_level(true, 99), 0);
    }
}
