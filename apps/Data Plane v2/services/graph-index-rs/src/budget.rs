//! The organization's processing policy and background budget, as graph-index reads them
//! (design task CR-T12): whether graph extraction is on for an organization, and what to do
//! when its background LLM spend for the month passes its budget.
//!
//! ## The owner's rulings this encodes
//!
//! * The budget is configurable per organization (`org_processing_policy.monthly_budget_usd`
//!   and `budget_mode`) and globally (`BACKGROUND_BUDGET_MODE`, `BACKGROUND_BUDGET_MONTHLY_USD`),
//!   and **the default is alert-only**: exceeding a budget alerts, it does not stop work. No
//!   figure is built in; the owner supplies it later (D-E).
//! * `stop` is explicit, operator-set and audited, and when it fires the work is not silently
//!   dropped: the chunks carry `skipped_budget`, a status that says why.
//! * `throttle` defers instead of skipping: an over-budget organization is worked one chunk at
//!   a time, never silently stopped.
//!
//! ## What "spend" is
//!
//! The Data Plane records tokens, not dollars: cost-core prices them from its catalogue, and the
//! catalogue is not read here. So the gate prices the month's `llm` tokens in `cost_events` with
//! a price table the deployment supplies (`BACKGROUND_PRICE_INPUT_USD_PER_MTOK` and
//! `..._OUTPUT_...`), with no default. Without one the spend in dollars is **unmeasurable**, and
//! the gate says so loudly and lets the work through (fail open, never silent): the policy API
//! refuses to store a budget figure until every consumer honours it (`policy.Enforced.Budget`
//! in data-quality-go), so an unmeasurable budget is only reachable by writing the table by
//! hand, and then the log line says what is missing.
//!
//! The spend counted is every background `llm` event of the organization (contextual retrieval,
//! graph extraction and community summaries), because the budget is the organization's
//! background LLM budget, not graph-index's share of it.

use std::fmt;

/// `org_processing_policy.graph_extraction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Inherit,
    On,
    Off,
}

impl Switch {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "inherit" => Some(Self::Inherit),
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// What to do when the budget is used up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetMode {
    /// Alert, and keep working. The system default.
    AlertOnly,
    /// Keep working, one chunk at a time.
    Throttle,
    /// Stop: the chunks are `skipped_budget`.
    Stop,
}

impl BudgetMode {
    /// A global mode: `alert_only`, `throttle` or `stop`. `inherit` is not a global value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "alert_only" => Some(Self::AlertOnly),
            "throttle" => Some(Self::Throttle),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlertOnly => "alert_only",
            Self::Throttle => "throttle",
            Self::Stop => "stop",
        }
    }
}

/// An organization's row of `org_processing_policy`, or the default of an absent row (everything
/// inherited): graph extraction on, the global budget.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrgPolicy {
    pub graph_extraction: Switch,
    /// `None`: `inherit`.
    pub budget_mode: Option<BudgetMode>,
    pub monthly_budget_usd: Option<f64>,
}

impl OrgPolicy {
    /// What an organization with no row has.
    pub const DEFAULT: OrgPolicy = OrgPolicy {
        graph_extraction: Switch::Inherit,
        budget_mode: None,
        monthly_budget_usd: None,
    };

    /// Whether graph extraction is turned off for the organization. `inherit` and `on` are both
    /// on: graph extraction has no global switch, and an absent row means the system default.
    pub fn graph_extraction_is_off(&self) -> bool {
        self.graph_extraction == Switch::Off
    }
}

/// What dollars a million tokens cost, from the deployment. Never defaulted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PriceTable {
    pub input_usd_per_mtok: f64,
    pub output_usd_per_mtok: f64,
}

impl PriceTable {
    pub fn spend_usd(&self, input_tokens: i64, output_tokens: i64) -> f64 {
        (input_tokens.max(0) as f64 * self.input_usd_per_mtok
            + output_tokens.max(0) as f64 * self.output_usd_per_mtok)
            / 1_000_000.0
    }
}

/// The deployment-wide budget: the mode an organization inherits, the figure an organization
/// without one inherits, and the prices that turn tokens into dollars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalBudget {
    pub mode: BudgetMode,
    pub monthly_usd: Option<f64>,
    pub price: Option<PriceTable>,
}

impl Default for GlobalBudget {
    fn default() -> Self {
        Self {
            mode: BudgetMode::AlertOnly,
            monthly_usd: None,
            price: None,
        }
    }
}

/// A deployment's budget settings that cannot work, with the variable to fix.
#[derive(Debug, PartialEq, Eq)]
pub struct BudgetConfigError(pub String);

impl fmt::Display for BudgetConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BudgetConfigError {}

impl GlobalBudget {
    /// Read the four settings (as the strings the environment holds; blank means unset) and
    /// refuse a combination that cannot work.
    pub fn parse(
        mode: &str,
        monthly_usd: &str,
        input_price: &str,
        output_price: &str,
    ) -> Result<Self, BudgetConfigError> {
        let number = |name: &str, value: &str| -> Result<Option<f64>, BudgetConfigError> {
            let value = value.trim();
            if value.is_empty() {
                return Ok(None);
            }
            match value.parse::<f64>() {
                Ok(parsed) if parsed.is_finite() && parsed >= 0.0 => Ok(Some(parsed)),
                _ => Err(BudgetConfigError(format!(
                    "{name} must be a number from 0 up, got `{value}`"
                ))),
            }
        };
        let mode_text = mode.trim();
        let mode = if mode_text.is_empty() {
            BudgetMode::AlertOnly
        } else {
            BudgetMode::parse(mode_text).ok_or_else(|| {
                BudgetConfigError(format!(
                    "BACKGROUND_BUDGET_MODE must be alert_only, throttle or stop, got `{mode_text}`"
                ))
            })?
        };
        let monthly_usd = number("BACKGROUND_BUDGET_MONTHLY_USD", monthly_usd)?;
        let input = number("BACKGROUND_PRICE_INPUT_USD_PER_MTOK", input_price)?;
        let output = number("BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK", output_price)?;
        let price =
            match (input, output) {
                (Some(input_usd_per_mtok), Some(output_usd_per_mtok)) => Some(PriceTable {
                    input_usd_per_mtok,
                    output_usd_per_mtok,
                }),
                (None, None) => None,
                _ => return Err(BudgetConfigError(
                    "BACKGROUND_PRICE_INPUT_USD_PER_MTOK and BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK \
                     are set together or not at all: half a price table prices nothing"
                        .to_owned(),
                )),
            };
        // A hard stop on a figure nothing can measure would be a stop that never fires: refuse
        // it at boot rather than let an operator believe the budget is enforced.
        if mode == BudgetMode::Stop && monthly_usd.is_some() && price.is_none() {
            return Err(BudgetConfigError(
                "BACKGROUND_BUDGET_MODE=stop with BACKGROUND_BUDGET_MONTHLY_USD needs the price \
                 table (BACKGROUND_PRICE_INPUT_USD_PER_MTOK and BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK): \
                 without it the spend cannot be measured and the stop would never fire"
                    .to_owned(),
            ));
        }
        Ok(Self {
            mode,
            monthly_usd,
            price,
        })
    }
}

/// What the gate decides for an organization right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    /// No budget applies, or it is not used up.
    Proceed,
    /// Over budget in `alert_only`: alert, and proceed.
    Alert { spend_usd: f64, budget_usd: f64 },
    /// A budget applies and its spend cannot be measured (no price table, or the ledger cannot
    /// be read). Alert, and proceed: never silent, never a stop on a guess.
    Unmeasured { budget_usd: f64 },
    /// Over budget in `throttle`: one chunk at a time.
    Throttle { spend_usd: f64, budget_usd: f64 },
    /// Over budget in `stop`: do not extract.
    Stop { spend_usd: f64, budget_usd: f64 },
}

/// The mode and the figure that apply to an organization: its own where it set them, the
/// deployment's otherwise.
pub fn effective(org: &OrgPolicy, global: &GlobalBudget) -> (BudgetMode, Option<f64>) {
    (
        org.budget_mode.unwrap_or(global.mode),
        org.monthly_budget_usd.or(global.monthly_usd),
    )
}

/// Whether the gate needs the month's spend at all: only an organization with a figure does.
pub fn needs_spend(org: &OrgPolicy, global: &GlobalBudget) -> bool {
    effective(org, global).1.is_some()
}

/// The decision. Pure: the caller supplies the spend (`None` when it could not be measured).
///
/// A budget of exactly zero is a budget that is used up before anything is spent, and `stop`
/// with it means "do not run": the comparison is `spend >= budget`.
pub fn decide(org: &OrgPolicy, global: &GlobalBudget, spend_usd: Option<f64>) -> Verdict {
    let (mode, figure) = effective(org, global);
    let Some(budget_usd) = figure else {
        return Verdict::Proceed;
    };
    let Some(spend_usd) = spend_usd else {
        return Verdict::Unmeasured { budget_usd };
    };
    if spend_usd < budget_usd {
        return Verdict::Proceed;
    }
    match mode {
        BudgetMode::AlertOnly => Verdict::Alert {
            spend_usd,
            budget_usd,
        },
        BudgetMode::Throttle => Verdict::Throttle {
            spend_usd,
            budget_usd,
        },
        BudgetMode::Stop => Verdict::Stop {
            spend_usd,
            budget_usd,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn org(mode: Option<BudgetMode>, figure: Option<f64>) -> OrgPolicy {
        OrgPolicy {
            graph_extraction: Switch::Inherit,
            budget_mode: mode,
            monthly_budget_usd: figure,
        }
    }

    fn global(mode: BudgetMode, figure: Option<f64>) -> GlobalBudget {
        GlobalBudget {
            mode,
            monthly_usd: figure,
            price: None,
        }
    }

    #[test]
    fn the_switch_and_the_mode_read_exactly_what_the_table_stores() {
        for (text, switch) in [
            ("inherit", Switch::Inherit),
            ("on", Switch::On),
            ("off", Switch::Off),
        ] {
            assert_eq!(Switch::parse(text), Some(switch));
        }
        assert_eq!(Switch::parse("OFF"), None, "the CHECK is case sensitive");
        assert_eq!(Switch::parse(""), None);
        for (text, mode) in [
            ("alert_only", BudgetMode::AlertOnly),
            ("throttle", BudgetMode::Throttle),
            ("stop", BudgetMode::Stop),
        ] {
            assert_eq!(BudgetMode::parse(text), Some(mode));
            assert_eq!(mode.as_str(), text);
        }
        assert_eq!(BudgetMode::parse("inherit"), None, "inherit is not a mode");
    }

    #[test]
    fn an_absent_policy_row_is_graph_extraction_on_and_nothing_else() {
        assert!(!OrgPolicy::DEFAULT.graph_extraction_is_off());
        assert!(!org(None, None).graph_extraction_is_off());
        let off = OrgPolicy {
            graph_extraction: Switch::Off,
            ..OrgPolicy::DEFAULT
        };
        assert!(off.graph_extraction_is_off());
        let on = OrgPolicy {
            graph_extraction: Switch::On,
            ..OrgPolicy::DEFAULT
        };
        assert!(!on.graph_extraction_is_off());
    }

    /// Owner default D-E: exceeding a budget alerts and does not stop. No figure is built in.
    #[test]
    fn the_default_never_stops_and_has_no_figure() {
        let default = GlobalBudget::default();
        assert_eq!(default.mode, BudgetMode::AlertOnly);
        assert_eq!(default.monthly_usd, None);
        assert_eq!(default.price, None);
        // With no figure anywhere there is nothing to exceed, whatever the spend.
        assert_eq!(
            decide(&OrgPolicy::DEFAULT, &default, Some(1e12)),
            Verdict::Proceed
        );
        assert_eq!(
            decide(&OrgPolicy::DEFAULT, &default, None),
            Verdict::Proceed
        );
        assert!(!needs_spend(&OrgPolicy::DEFAULT, &default));
        // And with a figure and the default mode, being over it alerts and proceeds.
        assert_eq!(
            decide(&org(None, Some(5.0)), &default, Some(7.5)),
            Verdict::Alert {
                spend_usd: 7.5,
                budget_usd: 5.0
            }
        );
    }

    #[test]
    fn under_the_figure_everything_proceeds_in_every_mode() {
        for mode in [
            BudgetMode::AlertOnly,
            BudgetMode::Throttle,
            BudgetMode::Stop,
        ] {
            assert_eq!(
                decide(
                    &org(Some(mode), Some(10.0)),
                    &GlobalBudget::default(),
                    Some(9.99)
                ),
                Verdict::Proceed,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn over_the_figure_each_mode_does_what_the_owner_ruled() {
        let over = |mode| {
            decide(
                &org(Some(mode), Some(10.0)),
                &GlobalBudget::default(),
                Some(10.0),
            )
        };
        assert!(matches!(over(BudgetMode::AlertOnly), Verdict::Alert { .. }));
        assert!(matches!(
            over(BudgetMode::Throttle),
            Verdict::Throttle { .. }
        ));
        assert!(matches!(over(BudgetMode::Stop), Verdict::Stop { .. }));
        // Exactly the figure is used up; a zero budget is used up before anything is spent.
        assert!(matches!(
            decide(
                &org(Some(BudgetMode::Stop), Some(0.0)),
                &GlobalBudget::default(),
                Some(0.0)
            ),
            Verdict::Stop { .. }
        ));
    }

    #[test]
    fn an_organization_inherits_the_deployments_mode_and_figure_unless_it_sets_its_own() {
        let deployment = global(BudgetMode::Stop, Some(100.0));
        // Nothing of its own: the deployment's mode and figure.
        assert_eq!(
            decide(&OrgPolicy::DEFAULT, &deployment, Some(150.0)),
            Verdict::Stop {
                spend_usd: 150.0,
                budget_usd: 100.0
            }
        );
        // Its own figure wins; its mode is still inherited.
        assert_eq!(
            decide(&org(None, Some(500.0)), &deployment, Some(150.0)),
            Verdict::Proceed
        );
        // Its own mode wins; the figure is inherited.
        assert!(matches!(
            decide(
                &org(Some(BudgetMode::AlertOnly), None),
                &deployment,
                Some(150.0)
            ),
            Verdict::Alert { .. }
        ));
        assert_eq!(
            effective(&org(None, None), &deployment),
            (BudgetMode::Stop, Some(100.0))
        );
    }

    /// A budget whose spend cannot be measured is never a stop on a guess and never silent.
    #[test]
    fn a_budget_that_cannot_be_measured_is_alerted_not_enforced() {
        for mode in [
            BudgetMode::AlertOnly,
            BudgetMode::Throttle,
            BudgetMode::Stop,
        ] {
            assert_eq!(
                decide(&org(Some(mode), Some(10.0)), &GlobalBudget::default(), None),
                Verdict::Unmeasured { budget_usd: 10.0 },
                "{mode:?}"
            );
        }
        assert!(needs_spend(
            &org(None, Some(10.0)),
            &GlobalBudget::default()
        ));
    }

    #[test]
    fn tokens_are_priced_by_the_table_the_deployment_supplies() {
        let price = PriceTable {
            input_usd_per_mtok: 0.15,
            output_usd_per_mtok: 0.60,
        };
        // 2,000,000 prompt tokens and 500,000 completion tokens.
        let spend = price.spend_usd(2_000_000, 500_000);
        assert!((spend - 0.6).abs() < 1e-9, "{spend}");
        assert_eq!(price.spend_usd(0, 0), 0.0);
        assert_eq!(price.spend_usd(-5, -5), 0.0, "a negative count is zero");
    }

    #[test]
    fn the_deployments_settings_are_read_and_a_combination_that_cannot_work_is_refused() {
        // Blank is unset: what compose passes for `${VAR:-}`.
        assert_eq!(
            GlobalBudget::parse("", "", "", "").unwrap(),
            GlobalBudget::default()
        );
        assert_eq!(
            GlobalBudget::parse("throttle", "25", "0.15", "0.6").unwrap(),
            GlobalBudget {
                mode: BudgetMode::Throttle,
                monthly_usd: Some(25.0),
                price: Some(PriceTable {
                    input_usd_per_mtok: 0.15,
                    output_usd_per_mtok: 0.6
                }),
            }
        );
        for (mode, figure, input, output, names) in [
            ("sometimes", "", "", "", "BACKGROUND_BUDGET_MODE"),
            ("", "-1", "", "", "BACKGROUND_BUDGET_MONTHLY_USD"),
            ("", "lots", "", "", "BACKGROUND_BUDGET_MONTHLY_USD"),
            ("", "NaN", "", "", "BACKGROUND_BUDGET_MONTHLY_USD"),
            ("", "", "0.15", "", "set together or not at all"),
            ("", "", "", "0.6", "set together or not at all"),
            (
                "",
                "",
                "-0.15",
                "0.6",
                "BACKGROUND_PRICE_INPUT_USD_PER_MTOK",
            ),
            ("stop", "25", "", "", "the stop would never fire"),
        ] {
            let error = GlobalBudget::parse(mode, figure, input, output)
                .expect_err(&format!("{mode}/{figure}/{input}/{output} cannot work"));
            assert!(error.to_string().contains(names), "{error}");
        }
        // A stop with no figure has nothing to exceed, and a stop with a figure and prices works.
        assert!(GlobalBudget::parse("stop", "", "", "").is_ok());
        assert!(GlobalBudget::parse("stop", "25", "0.15", "0.6").is_ok());
    }
}
