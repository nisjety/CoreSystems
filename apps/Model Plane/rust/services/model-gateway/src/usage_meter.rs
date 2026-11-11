//! The run-cost meter's arithmetic. Pure: no clock, no I/O, no catalogue. A caller
//! prices each provider call against the pricing catalogue, records the call
//! here, and reads back the run's running total.
//!
//! The total is honest or absent. Once any metered call is unpriced the cost is
//! `None` for the rest of the run, never a sum of the calls that happened to be
//! priced. A call served by a subscription adds its tokens and no dollars.

use mp_contracts::model_plane::v1::InferPurpose;

use crate::pricing::{CategoryCost, CostBasis, Priced, TokenLegs};

/// What a provider call was for. The tokens are the closed vocabulary a progress
/// frame names the latest call by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallPhase {
    /// The streamed answer.
    Answer,
    /// A tool-decision round of the chat tool loop.
    ToolRound,
    /// A round of a Do run.
    AgentRound,
    /// A provider call the turn makes about itself (title, follow-ups,
    /// validation) rather than for its answer.
    Overhead,
}

impl CallPhase {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::ToolRound => "tool_round",
            Self::AgentRound => "agent_round",
            Self::Overhead => "overhead",
        }
    }

    /// The purpose a call of this phase is counted under. `Overhead` covers the side
    /// tasks a turn makes about itself (title, follow-ups, judge), and each of those
    /// names its own purpose on its request, so the phase alone names none.
    #[must_use]
    pub fn purpose(self) -> InferPurpose {
        match self {
            Self::Answer => InferPurpose::Answer,
            Self::ToolRound => InferPurpose::ToolRound,
            Self::AgentRound => InferPurpose::AgentRound,
            Self::Overhead => InferPurpose::Unspecified,
        }
    }
}

/// One provider call, as reported. `input` folds in both cache legs, exactly as
/// the provider's `input_tokens` does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallUsage {
    pub phase: CallPhase,
    pub model: String,
    pub provider: String,
    pub basis: CostBasis,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    /// Served on the priority tier (`InferResponse.service_tier_used`), so a metered price is multiplied
    /// by the priority-tier multiplier (`pricing::priority_tier_cost_multiplier`). `false` for every call
    /// the provider did not serve on that tier.
    pub priority: bool,
}

impl CallUsage {
    /// The counts to price this call with.
    #[must_use]
    pub fn legs(&self) -> TokenLegs {
        TokenLegs {
            input_tokens: self.input,
            output_tokens: self.output,
            cache_read_input_tokens: self.cache_read,
            cache_creation_input_tokens: self.cache_write,
        }
    }
}

/// The run so far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub calls: u32,
    /// Input tokens including both cache legs, as the wire states them.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// The metered calls' cost. `None` when any metered call was unpriced and
    /// when no metered call has been priced: a subscription-only run has no
    /// dollar figure, not a zero one.
    pub cost_usd: Option<f64>,
    /// Metered calls whose price was unknown.
    pub unpriced_calls: u32,
    /// `None` before the first call and when metered and subscription calls
    /// are mixed.
    pub basis: Option<CostBasis>,
    /// The parts of `cost_usd`; present exactly when it is.
    pub by_category: Option<CategoryCost>,
}

impl Snapshot {
    /// Input tokens that were neither read from nor written to the prompt
    /// cache.
    #[must_use]
    pub fn uncached_input(&self) -> u64 {
        self.input
            .saturating_sub(self.cache_read)
            .saturating_sub(self.cache_write)
    }
}

/// Accumulates a run's provider calls. Every counter saturates.
#[derive(Clone, Debug, Default)]
pub struct UsageMeter {
    calls: u32,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    metered_priced: u32,
    unpriced_calls: u32,
    subscription_calls: u32,
    usd: f64,
    by_category: CategoryCost,
}

/// A reported count as a counter increment: a negative count adds nothing.
fn count(tokens: i64) -> u64 {
    tokens.max(0).unsigned_abs()
}

impl UsageMeter {
    /// Record one call priced as `priced` and return the run so far. `priced`
    /// is ignored for a subscription call, and a metered call counts as priced
    /// only when it carries both a finite cost and its categories.
    pub fn record(&mut self, call: &CallUsage, priced: &Priced) -> Snapshot {
        self.calls = self.calls.saturating_add(1);
        self.input = self.input.saturating_add(count(call.input));
        self.output = self.output.saturating_add(count(call.output));
        self.cache_read = self.cache_read.saturating_add(count(call.cache_read));
        self.cache_write = self.cache_write.saturating_add(count(call.cache_write));
        match call.basis {
            CostBasis::Subscription => {
                self.subscription_calls = self.subscription_calls.saturating_add(1);
            }
            CostBasis::Metered => match (priced.usd, priced.by_category) {
                (Some(usd), Some(categories)) if usd.is_finite() => {
                    self.metered_priced = self.metered_priced.saturating_add(1);
                    self.usd += usd;
                    self.by_category = self.by_category.plus(categories);
                }
                _ => self.unpriced_calls = self.unpriced_calls.saturating_add(1),
            },
        }
        self.snapshot()
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let priced = self.unpriced_calls == 0 && self.metered_priced > 0;
        let basis = match (
            self.metered_priced > 0 || self.unpriced_calls > 0,
            self.subscription_calls > 0,
        ) {
            (true, false) => Some(CostBasis::Metered),
            (false, true) => Some(CostBasis::Subscription),
            _ => None,
        };
        Snapshot {
            calls: self.calls,
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
            cost_usd: priced.then_some(self.usd),
            unpriced_calls: self.unpriced_calls,
            basis,
            by_category: priced.then_some(self.by_category),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::PriceSource;

    #[test]
    fn each_call_phase_names_the_purpose_it_is_counted_under() {
        assert_eq!(CallPhase::Answer.purpose(), InferPurpose::Answer);
        assert_eq!(CallPhase::ToolRound.purpose(), InferPurpose::ToolRound);
        assert_eq!(CallPhase::AgentRound.purpose(), InferPurpose::AgentRound);
        assert_eq!(CallPhase::Overhead.purpose(), InferPurpose::Unspecified);
    }

    fn call(basis: CostBasis, input: i64, output: i64, read: i64, write: i64) -> CallUsage {
        CallUsage {
            phase: CallPhase::ToolRound,
            model: "m".into(),
            provider: "p".into(),
            basis,
            input,
            output,
            cache_read: read,
            cache_write: write,
            priority: false,
        }
    }

    fn priced(usd: f64) -> Priced {
        Priced {
            usd: Some(usd),
            by_category: Some(CategoryCost {
                input_usd: usd / 2.0,
                cache_read_usd: 0.0,
                cache_write_usd: 0.0,
                output_usd: usd / 2.0,
            }),
            source: PriceSource::Exact,
            note: None,
        }
    }

    fn unpriced() -> Priced {
        Priced {
            usd: None,
            by_category: None,
            source: PriceSource::Default,
            note: None,
        }
    }

    #[test]
    fn an_empty_meter_states_nothing() {
        let snapshot = UsageMeter::default().snapshot();
        assert_eq!(snapshot.calls, 0);
        assert_eq!(snapshot.cost_usd, None);
        assert_eq!(snapshot.basis, None);
        assert_eq!(snapshot.by_category, None);
    }

    #[test]
    fn priced_metered_calls_sum_and_so_do_their_categories() {
        let mut meter = UsageMeter::default();
        meter.record(&call(CostBasis::Metered, 1_000, 100, 0, 0), &priced(0.25));
        let snapshot = meter.record(&call(CostBasis::Metered, 500, 50, 200, 100), &priced(0.5));
        assert_eq!(snapshot.calls, 2);
        assert_eq!(snapshot.input, 1_500);
        assert_eq!(snapshot.output, 150);
        assert_eq!(snapshot.cache_read, 200);
        assert_eq!(snapshot.cache_write, 100);
        assert_eq!(snapshot.uncached_input(), 1_200);
        assert_eq!(snapshot.cost_usd, Some(0.75));
        assert_eq!(snapshot.basis, Some(CostBasis::Metered));
        let parts = snapshot.by_category.expect("categories of a priced run");
        assert!((parts.total() - 0.75).abs() < 1e-12);
    }

    #[test]
    fn any_unpriced_metered_call_nulls_the_total() {
        let mut meter = UsageMeter::default();
        let first = meter.record(&call(CostBasis::Metered, 1_000, 100, 0, 0), &priced(0.25));
        assert_eq!(first.cost_usd, Some(0.25));

        let second = meter.record(&call(CostBasis::Metered, 900, 90, 0, 0), &unpriced());
        assert_eq!(second.cost_usd, None, "never a partial sum");
        assert_eq!(second.by_category, None);
        assert_eq!(second.unpriced_calls, 1);
        assert_eq!(second.calls, 2);
        assert_eq!(second.input, 1_900, "the tokens still count");

        // A later priced call does not bring the figure back.
        let third = meter.record(&call(CostBasis::Metered, 10, 1, 0, 0), &priced(0.01));
        assert_eq!(third.cost_usd, None);
        assert_eq!(third.unpriced_calls, 1);

        // A metered call whose price lacks a figure or its parts is unpriced,
        // and so is a non-finite one.
        for bad in [
            Priced {
                usd: Some(0.1),
                by_category: None,
                source: PriceSource::Exact,
                note: None,
            },
            Priced {
                usd: Some(f64::NAN),
                by_category: priced(0.1).by_category,
                source: PriceSource::Exact,
                note: None,
            },
        ] {
            let mut meter = UsageMeter::default();
            let got = meter.record(&call(CostBasis::Metered, 1, 1, 0, 0), &bad);
            assert_eq!((got.cost_usd, got.unpriced_calls), (None, 1), "{bad:?}");
        }
    }

    #[test]
    fn subscription_calls_add_tokens_and_no_usd() {
        let mut meter = UsageMeter::default();
        // A subscription call's `Priced` carries a known zero; it must not
        // read as spend.
        let zero = Priced {
            usd: Some(0.0),
            by_category: None,
            source: PriceSource::Subscription,
            note: None,
        };
        let only = meter.record(
            &call(CostBasis::Subscription, 1_800, 120, 1_024, 256),
            &zero,
        );
        assert_eq!(only.calls, 1);
        assert_eq!(only.input, 1_800);
        assert_eq!(only.output, 120);
        assert_eq!(only.cost_usd, None, "included in the subscription, not $0");
        assert_eq!(only.by_category, None);
        assert_eq!(only.unpriced_calls, 0);
        assert_eq!(only.basis, Some(CostBasis::Subscription));

        // Even a nonzero price attached to a subscription call is not spend.
        let mut meter = UsageMeter::default();
        let ignored = meter.record(&call(CostBasis::Subscription, 10, 1, 0, 0), &priced(9.0));
        assert_eq!(ignored.cost_usd, None);

        // Beside a priced metered call the subscription call adds tokens only.
        let mut meter = UsageMeter::default();
        meter.record(&call(CostBasis::Metered, 1_000, 100, 0, 0), &priced(0.25));
        let mixed = meter.record(&call(CostBasis::Subscription, 500, 50, 0, 0), &zero);
        assert_eq!(mixed.cost_usd, Some(0.25));
        assert_eq!(mixed.input, 1_500);
        assert_eq!(mixed.basis, None, "mixed bases state no basis");
        assert_eq!(mixed.unpriced_calls, 0);
    }

    #[test]
    fn a_subscription_call_does_not_hide_an_unpriced_metered_one() {
        let mut meter = UsageMeter::default();
        meter.record(
            &call(CostBasis::Subscription, 10, 1, 0, 0),
            &Priced {
                usd: Some(0.0),
                by_category: None,
                source: PriceSource::Subscription,
                note: None,
            },
        );
        let got = meter.record(&call(CostBasis::Metered, 10, 1, 0, 0), &unpriced());
        assert_eq!(got.cost_usd, None);
        assert_eq!(got.unpriced_calls, 1);
    }

    #[test]
    fn uncached_input_saturates_and_counters_saturate() {
        let mut meter = UsageMeter::default();
        // Legs larger than the input are a provider quirk, not a negative.
        let odd = meter.record(&call(CostBasis::Metered, 100, 1, 80, 80), &priced(0.1));
        assert_eq!(odd.uncached_input(), 0);

        // A negative count adds nothing.
        let mut meter = UsageMeter::default();
        let negative = meter.record(&call(CostBasis::Metered, -5, -1, -2, -3), &priced(0.1));
        assert_eq!(
            (
                negative.input,
                negative.output,
                negative.cache_read,
                negative.cache_write
            ),
            (0, 0, 0, 0)
        );

        let mut meter = UsageMeter::default();
        meter.record(
            &call(CostBasis::Metered, i64::MAX, i64::MAX, 0, 0),
            &priced(0.1),
        );
        meter.record(
            &call(CostBasis::Metered, i64::MAX, i64::MAX, 0, 0),
            &priced(0.1),
        );
        let big = meter.record(
            &call(CostBasis::Metered, i64::MAX, i64::MAX, 0, 0),
            &priced(0.1),
        );
        assert_eq!(big.input, u64::MAX, "three i64::MAX inputs saturate");
        assert_eq!(big.output, u64::MAX);

        let mut saturated = UsageMeter {
            calls: u32::MAX,
            unpriced_calls: u32::MAX,
            ..UsageMeter::default()
        };
        let got = saturated.record(&call(CostBasis::Metered, 1, 1, 0, 0), &unpriced());
        assert_eq!((got.calls, got.unpriced_calls), (u32::MAX, u32::MAX));
    }

    #[test]
    fn phases_have_closed_tokens() {
        let names: Vec<&str> = [
            CallPhase::Answer,
            CallPhase::ToolRound,
            CallPhase::AgentRound,
            CallPhase::Overhead,
        ]
        .into_iter()
        .map(CallPhase::as_str)
        .collect();
        assert_eq!(names, ["answer", "tool_round", "agent_round", "overhead"]);
    }

    #[test]
    fn a_call_prices_with_its_own_counts() {
        let usage = call(CostBasis::Metered, 8_520, 42, 8_000, 0);
        assert_eq!(
            usage.legs(),
            TokenLegs {
                input_tokens: 8_520,
                output_tokens: 42,
                cache_read_input_tokens: 8_000,
                cache_creation_input_tokens: 0,
            }
        );
    }
}
