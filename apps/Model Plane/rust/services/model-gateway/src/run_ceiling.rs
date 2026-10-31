//! The per-run ceiling (WS27-S6): the spend and token caps a turn may use, the
//! 80% and reached warnings on the way to them, and the switch that stops the
//! tool phase once a cap is reached.
//!
//! Pure: no clock, no I/O, no catalogue. The turn's progress sink records each
//! provider call here, and the tool loop asks it before it starts another round.
//!
//! The precedence is the one `budget.rs` applies: a cap on the request is used
//! where it sets one, and the org's configured ceiling fills whatever the request
//! left unset. Only metered calls count toward the token cap, as cost-core counts
//! them, and a subscription call adds no spend, so a subscription turn is never
//! warned or stopped.

/// Switch for the 80% and reached warnings on `usage_progress` frames. Only the
/// literal `true` enables it, and the turn reads it once, when it resolves its
/// ceiling.
pub const WARN_ENV: &str = "MODEL_GATEWAY_RUN_CEILING_WARN";
/// Switch that ends the tool phase once the run has reached a cap. Only the
/// literal `true` enables it, and the turn reads it once, as above.
pub const ENFORCE_ENV: &str = "MODEL_GATEWAY_RUN_CEILING_ENFORCE";

/// The share of a cap at which the first warning is due.
const EIGHTY_PERCENT: f64 = 0.8;
/// The share of a cap at which the run has reached it.
const REACHED: f64 = 1.0;

/// Whether the warnings switch is on.
#[must_use]
pub fn warn_enabled(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    lookup(WARN_ENV).as_deref() == Some("true")
}

/// Whether the enforcement switch is on.
#[must_use]
pub fn enforce_enabled(lookup: &dyn Fn(&str) -> Option<String>) -> bool {
    lookup(ENFORCE_ENV).as_deref() == Some("true")
}

/// The caps one turn may spend. A dimension left `None` is uncapped.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunCeiling {
    /// The metered spend cap, in US dollars.
    pub usd: Option<f64>,
    /// The metered token cap: the input and output tokens of metered calls.
    pub tokens: Option<u64>,
}

impl RunCeiling {
    /// The ceiling for a turn. A cap on the request wins where it sets one, and
    /// the org's ceiling fills what the request left unset. A cap that is not
    /// positive is no cap, which is how `org_quota` reads one.
    #[must_use]
    pub fn resolve(
        request_usd: Option<f64>,
        request_tokens: Option<u32>,
        org_usd: Option<f64>,
        org_tokens: Option<i64>,
    ) -> Self {
        let usd = positive_usd(request_usd).or_else(|| positive_usd(org_usd));
        let tokens = request_tokens
            .filter(|cap| *cap > 0)
            .map(u64::from)
            .or_else(|| org_tokens.and_then(|cap| u64::try_from(cap).ok()))
            .filter(|cap| *cap > 0);
        Self { usd, tokens }
    }

    /// Whether any dimension is capped.
    #[must_use]
    pub fn is_set(self) -> bool {
        self.usd.is_some() || self.tokens.is_some()
    }

    /// The share of the ceiling the run has used: the larger share among the
    /// capped dimensions whose figure is known. `None` when none of them has a
    /// known figure, so an unpriced call is never read as a share of zero.
    #[must_use]
    pub fn share(self, cost_usd: Option<f64>, metered_tokens: u64) -> Option<f64> {
        let usd = self.usd.zip(cost_usd).map(|(cap, spent)| spent / cap);
        let tokens = self.tokens.map(|cap| ratio(metered_tokens, cap));
        usd.into_iter().chain(tokens).reduce(f64::max)
    }
}

fn positive_usd(cap: Option<f64>) -> Option<f64> {
    cap.filter(|cap| cap.is_finite() && *cap > 0.0)
}

/// A token count as a share of a token cap. Counts stay far below 2^53, where
/// the conversion to a float is exact.
#[allow(clippy::cast_precision_loss)]
fn ratio(count: u64, cap: u64) -> f64 {
    count as f64 / cap as f64
}

/// A warning a progress frame carries the first time its run crosses a threshold.
/// The variants are ordered, so the later threshold is the greater one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CeilingWarning {
    /// The run has used 80% of a cap.
    Eighty,
    /// The run has reached a cap.
    Reached,
}

impl CeilingWarning {
    /// The closed token a frame names the warning by.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eighty => "run_ceiling_80",
            Self::Reached => "run_ceiling_reached",
        }
    }
}

/// The warning due at `share`, when the run was last sent `sent`. A threshold is
/// warned once. A run that jumps from under 80% straight past its cap gets only
/// the reached warning, since that is the one the reader needs.
#[must_use]
pub fn warning_due(share: f64, sent: Option<CeilingWarning>) -> Option<CeilingWarning> {
    let due = if share >= REACHED {
        CeilingWarning::Reached
    } else if share >= EIGHTY_PERCENT {
        CeilingWarning::Eighty
    } else {
        return None;
    };
    sent.is_none_or(|sent| due > sent).then_some(due)
}

/// What a progress frame states about its turn's ceiling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CeilingProgress {
    /// The metered spend cap, when one is set.
    pub ceiling_usd: Option<f64>,
    /// The metered token cap, when one is set.
    pub ceiling_tokens: Option<u64>,
    /// The share of the ceiling used, when a capped figure is known.
    pub fraction: Option<f64>,
    /// The warning this frame is the first to carry, if it carries one.
    pub warning: Option<CeilingWarning>,
}

/// One turn's ceiling as its progress sink keeps it: the caps, the metered tokens
/// counted so far, and the warning already sent. It exists only when something
/// acts on the ceiling, so a turn with both switches off keeps no state at all.
#[derive(Clone, Debug, PartialEq)]
pub struct CeilingTracker {
    ceiling: RunCeiling,
    /// Whether the progress frames carry the ceiling.
    report: bool,
    /// Whether reaching the ceiling ends the tool phase.
    enforce: bool,
    metered_tokens: u64,
    sent: Option<CeilingWarning>,
}

impl CeilingTracker {
    /// The tracker for a turn, or `None` when nothing acts on the ceiling: no cap
    /// is set, or neither the frames nor the enforcement is switched on.
    #[must_use]
    pub fn for_turn(ceiling: RunCeiling, report: bool, enforce: bool) -> Option<Self> {
        (ceiling.is_set() && (report || enforce)).then_some(Self {
            ceiling,
            report,
            enforce,
            metered_tokens: 0,
            sent: None,
        })
    }

    /// Counts one call's metered tokens into the run, then states the run's
    /// progress with that call included.
    pub fn record(&mut self, call_tokens: u64, cost_usd: Option<f64>) -> Option<CeilingProgress> {
        self.metered_tokens = self.metered_tokens.saturating_add(call_tokens);
        self.progress(cost_usd, self.metered_tokens)
    }

    /// States the progress of a total the caller has summed itself, such as the
    /// turn's closing total, which also counts the answer. Does not change the
    /// count the tracker stops on.
    pub fn progress(
        &mut self,
        cost_usd: Option<f64>,
        metered_tokens: u64,
    ) -> Option<CeilingProgress> {
        if !self.report {
            return None;
        }
        let fraction = self.ceiling.share(cost_usd, metered_tokens);
        let warning = fraction.and_then(|share| warning_due(share, self.sent));
        self.sent = warning.or(self.sent);
        Some(CeilingProgress {
            ceiling_usd: self.ceiling.usd,
            ceiling_tokens: self.ceiling.tokens,
            fraction,
            warning,
        })
    }

    /// Whether the tool phase ends before its next round: enforcement is on and
    /// the run, with `cost_usd` spent so far, has reached a cap.
    #[must_use]
    pub fn stops_tool_phase(&self, cost_usd: Option<f64>) -> bool {
        self.enforce
            && self
                .ceiling
                .share(cost_usd, self.metered_tokens)
                .is_some_and(|share| share >= REACHED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A share agrees with the expected one to within rounding.
    fn near(actual: Option<f64>, expected: f64) {
        assert!(
            actual.is_some_and(|share| (share - expected).abs() < 1e-9),
            "{actual:?} is not {expected}"
        );
    }

    #[test]
    fn request_cap_wins_and_org_fills_unset() {
        // The request's cap is used where it sets one, and the org's where it does not.
        assert_eq!(
            RunCeiling::resolve(Some(0.5), None, Some(5.0), Some(1_000)),
            RunCeiling {
                usd: Some(0.5),
                tokens: Some(1_000)
            }
        );
        assert_eq!(
            RunCeiling::resolve(None, Some(200), Some(5.0), None),
            RunCeiling {
                usd: Some(5.0),
                tokens: Some(200)
            }
        );
        // budget.rs sends the request's cap as it stands, even one above the org's. Whether
        // a request may loosen an org ceiling is an open decision, so this keeps the run
        // ceiling in agreement with the gate rather than settling that question here.
        assert_eq!(
            RunCeiling::resolve(Some(50.0), None, Some(5.0), None).usd,
            Some(50.0)
        );
        // A cap that is not positive is no cap, so the org's fills its place.
        assert_eq!(
            RunCeiling::resolve(Some(0.0), Some(0), Some(5.0), Some(0)),
            RunCeiling {
                usd: Some(5.0),
                tokens: None
            }
        );
        assert_eq!(
            RunCeiling::resolve(None, None, None, Some(-1)),
            RunCeiling::default()
        );
        assert!(!RunCeiling::resolve(None, None, None, None).is_set());
    }

    #[test]
    fn the_share_is_the_larger_known_share_and_an_unknown_figure_is_not_zero() {
        let both = RunCeiling {
            usd: Some(0.05),
            tokens: Some(1_000),
        };
        near(both.share(Some(0.02), 900), 0.9);
        near(both.share(Some(0.04), 100), 0.8);
        // An unpriced call leaves the spend unknown, and the token share still stands.
        near(both.share(None, 500), 0.5);
        // With no known figure there is no share to report.
        assert_eq!(
            RunCeiling {
                usd: Some(0.05),
                tokens: None
            }
            .share(None, 500),
            None
        );
        // Zero tokens against a token cap is a known share of zero.
        near(
            RunCeiling {
                usd: None,
                tokens: Some(1_000),
            }
            .share(None, 0),
            0.0,
        );
    }

    #[test]
    fn warning_fires_once_at_80_and_once_at_100() {
        assert_eq!(warning_due(0.79, None), None);
        assert_eq!(warning_due(0.8, None), Some(CeilingWarning::Eighty));
        assert_eq!(warning_due(0.95, Some(CeilingWarning::Eighty)), None);
        assert_eq!(
            warning_due(1.0, Some(CeilingWarning::Eighty)),
            Some(CeilingWarning::Reached)
        );
        assert_eq!(warning_due(1.4, Some(CeilingWarning::Reached)), None);
        // A run that jumps from under 80% straight past its cap is told only that it reached it.
        assert_eq!(warning_due(1.2, None), Some(CeilingWarning::Reached));
        assert_eq!(CeilingWarning::Eighty.as_str(), "run_ceiling_80");
        assert_eq!(CeilingWarning::Reached.as_str(), "run_ceiling_reached");
    }

    #[test]
    fn each_warning_rides_the_one_frame_that_first_carries_it() {
        let cap = RunCeiling {
            usd: Some(0.5),
            tokens: None,
        };
        let mut tracker =
            CeilingTracker::for_turn(cap, true, false).expect("a capped turn that reports");
        let half = tracker
            .record(0, Some(0.25))
            .expect("a reporting tracker states every frame");
        assert_eq!(
            (half.ceiling_usd, half.ceiling_tokens, half.warning),
            (Some(0.5), None, None)
        );
        near(half.fraction, 0.5);
        let eighty = tracker.record(0, Some(0.4)).expect("reports");
        assert_eq!(eighty.warning, Some(CeilingWarning::Eighty));
        let still_eighty = tracker.record(0, Some(0.45)).expect("reports");
        assert_eq!(still_eighty.warning, None, "the 80% warning is sent once");
        let reached = tracker.record(0, Some(0.5)).expect("reports");
        assert_eq!(reached.warning, Some(CeilingWarning::Reached));
        let past = tracker.record(0, Some(0.7)).expect("reports");
        assert_eq!(past.warning, None, "the reached warning is sent once");
        near(past.fraction, 1.4);
    }

    #[test]
    fn subscription_is_exempt() {
        // A subscription call adds neither spend nor metered tokens, so a capped
        // subscription turn is never warned or stopped, however many tokens it uses.
        let cap = RunCeiling {
            usd: Some(0.05),
            tokens: Some(1_000),
        };
        let mut turn = CeilingTracker::for_turn(cap, true, true).expect("a capped turn");
        for _ in 0..5 {
            let frame = turn.record(0, None).expect("reports");
            assert_eq!(frame.warning, None);
        }
        assert!(!turn.stops_tool_phase(None));
        // Beside metered calls, only their tokens count toward the cap: 600 of 1_000 is
        // below 80%, and 800 is at it.
        turn.record(600, Some(0.001));
        let mixed = turn.record(200, Some(0.002)).expect("reports");
        assert_eq!(mixed.warning, Some(CeilingWarning::Eighty));
    }

    #[test]
    fn enforcement_ends_tool_phase_and_still_answers() {
        let cap = RunCeiling {
            usd: Some(0.05),
            tokens: None,
        };
        let mut enforced =
            CeilingTracker::for_turn(cap, true, true).expect("an enforced, reporting cap");
        enforced.record(0, Some(0.04));
        assert!(
            !enforced.stops_tool_phase(Some(0.04)),
            "under the cap the tool phase goes on"
        );
        let over = enforced.record(0, Some(0.06)).expect("reports");
        assert_eq!(over.warning, Some(CeilingWarning::Reached));
        assert!(
            enforced.stops_tool_phase(Some(0.06)),
            "at the cap no further round starts"
        );
        // The answer is not blocked: its closing frame still states the ceiling, and the
        // reached warning, already sent, is not sent again.
        let answer = enforced
            .progress(Some(0.07), 0)
            .expect("the closing frame still reports");
        assert_eq!(answer.warning, None);
        near(answer.fraction, 1.4);
        // The switch decides the stop: with only the warning on, the cap is stated but not enforced.
        let mut warned = CeilingTracker::for_turn(cap, true, false).expect("a warning-only cap");
        warned.record(0, Some(0.06));
        assert!(!warned.stops_tool_phase(Some(0.06)));
        // Enforcement does not need the frames: a quiet, enforced tracker still stops the tool phase.
        let mut quiet = CeilingTracker::for_turn(cap, false, true).expect("an enforced cap");
        assert_eq!(quiet.record(0, Some(0.06)), None);
        assert!(quiet.stops_tool_phase(Some(0.06)));
    }

    #[test]
    fn a_tracker_exists_only_for_a_cap_that_something_acts_on() {
        let capped = RunCeiling {
            usd: Some(1.0),
            tokens: None,
        };
        assert!(
            CeilingTracker::for_turn(RunCeiling::default(), true, true).is_none(),
            "no cap"
        );
        assert!(
            CeilingTracker::for_turn(capped, false, false).is_none(),
            "neither switch"
        );
        assert!(CeilingTracker::for_turn(capped, true, false).is_some());
        assert!(CeilingTracker::for_turn(capped, false, true).is_some());
    }

    #[test]
    fn only_the_literal_true_enables_either_switch() {
        assert!(warn_enabled(&|key| {
            assert_eq!(key, WARN_ENV);
            Some("true".to_owned())
        }));
        assert!(!warn_enabled(&|_| Some("True".to_owned())));
        assert!(!warn_enabled(&|_| None));
        assert!(enforce_enabled(&|key| {
            assert_eq!(key, ENFORCE_ENV);
            Some("true".to_owned())
        }));
        assert!(!enforce_enabled(&|_| Some("1".to_owned())));
        assert!(!enforce_enabled(&|_| None));
    }
}
