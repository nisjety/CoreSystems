//! The round and delegation budget of one agentic (Do mode) run, resolved on
//! the server.
//!
//! `RunAgentRequest.max_rounds` and `max_subagents` used to be literals at the
//! dispatch site (`4` and `0`). They are now the output of ONE pure function,
//! [`resolve_run_budget`], so a run's budget has a single source and later
//! inputs (a plan's step count, a wide-research request) are added to that
//! function instead of as a second place that decides.
//!
//! # Never client-supplied
//!
//! Nothing here reads the request. The numbers come from the process
//! environment and from whether the turn is pinned to the ChatGPT subscription
//! (`AgentRoute`), which the gateway itself decided. A browser, an API client
//! or a model cannot widen a run's budget by what it sends.
//!
//! # Variables
//!
//! | variable | meaning | when unset |
//! |---|---|---|
//! | `AGENTIC_MAX_ROUNDS` | inference rounds of one run, parent and delegated children together | 4 |
//! | `AGENTIC_MAX_SUBAGENTS` | delegations one run may open | 0, which execution-core reads as "unspecified" and resolves to its own default |
//! | `AGENTIC_SUBSCRIPTION_MAX_ROUNDS` | the same, for a turn pinned to the subscription only | `AGENTIC_MAX_ROUNDS` |
//! | `AGENTIC_SUBSCRIPTION_MAX_SUBAGENTS` | the same, for a turn pinned to the subscription only | `AGENTIC_MAX_SUBAGENTS` |
//!
//! With none of them set the request is byte-identical to the one the gateway
//! sent before this module existed.
//!
//! A value that is empty, not a whole number, or (for rounds) zero is treated
//! as unset, never as an error: compose passes an unset variable as an empty
//! string. Rounds of zero are refused on purpose, because execution-core reads
//! `max_rounds = 0` as "use my default", which is 12: a typo must not triple a
//! run's budget. Subagents of zero is a valid value (execution-core's own
//! default applies).
//!
//! The total across a parent and its children stays bounded by `max_rounds`:
//! execution-core charges every delegated round to the same budget.
//!
//! The wall-clock side of a run's budget lives in execution-core
//! (`EXECUTION_RUN_WALL_CLOCK_SECS`), which is where the loop runs.

/// Rounds of one run, every route.
pub const MAX_ROUNDS_ENV: &str = "AGENTIC_MAX_ROUNDS";
/// Delegations of one run, every route.
pub const MAX_SUBAGENTS_ENV: &str = "AGENTIC_MAX_SUBAGENTS";
/// Rounds of one run pinned to the ChatGPT subscription. Wins over
/// [`MAX_ROUNDS_ENV`] there and is ignored everywhere else.
pub const SUBSCRIPTION_MAX_ROUNDS_ENV: &str = "AGENTIC_SUBSCRIPTION_MAX_ROUNDS";
/// Delegations of one run pinned to the ChatGPT subscription. Wins over
/// [`MAX_SUBAGENTS_ENV`] there and is ignored everywhere else.
pub const SUBSCRIPTION_MAX_SUBAGENTS_ENV: &str = "AGENTIC_SUBSCRIPTION_MAX_SUBAGENTS";

/// What the gateway sent before the budget was resolvable: four rounds.
pub const DEFAULT_MAX_ROUNDS: u32 = 4;
/// What the gateway sent before the budget was resolvable: unspecified.
pub const DEFAULT_MAX_SUBAGENTS: u32 = 0;

/// The most rounds the gateway will ask for. Mirrors execution-core's
/// `MAX_ROUNDS_CEILING`, which clamps again on its side, so a drift between the
/// two can only make a run shorter than configured, never longer.
pub const MAX_ROUNDS_CEILING: u32 = 32;
/// The most delegations the gateway will ask for. Mirrors execution-core's
/// `subagent::MAX_CHILDREN_CEILING`, with the same one-way safety.
pub const MAX_SUBAGENTS_CEILING: u32 = 100;

/// The budget of one run, in the units of `RunAgentRequest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunBudget {
    /// `RunAgentRequest.max_rounds`: at least 1.
    pub max_rounds: u32,
    /// `RunAgentRequest.max_subagents`: 0 means execution-core's default.
    pub max_subagents: u32,
}

impl Default for RunBudget {
    /// The literals the dispatch site used to carry.
    fn default() -> Self {
        Self {
            max_rounds: DEFAULT_MAX_ROUNDS,
            max_subagents: DEFAULT_MAX_SUBAGENTS,
        }
    }
}

/// A whole number of at least `min`, from a raw environment value.
fn parse_count(raw: Option<&str>, min: u32) -> Option<u32> {
    raw.map(str::trim)
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|&count| count >= min)
}

/// The first of `keys` (in order) that holds a usable value, clamped to
/// `ceiling`, else `default`.
fn first_usable(
    keys: &[&str],
    lookup: &dyn Fn(&str) -> Option<String>,
    min: u32,
    ceiling: u32,
    default: u32,
) -> u32 {
    keys.iter()
        .find_map(|key| parse_count(lookup(key).as_deref(), min))
        .map_or(default, |count| count.min(ceiling))
}

/// The budget of a run, from the environment (`lookup`) and the route the
/// gateway pinned the turn to. Pure: the environment arrives as an argument, so
/// a test never touches the process environment.
///
/// On a subscription route the `AGENTIC_SUBSCRIPTION_*` variable is tried
/// first and the general one second; off it only the general one is read. This
/// is the function B6 and C5 extend with their inputs.
#[must_use]
pub fn resolve_run_budget(
    on_subscription_route: bool,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> RunBudget {
    let rounds_keys: &[&str] = if on_subscription_route {
        &[SUBSCRIPTION_MAX_ROUNDS_ENV, MAX_ROUNDS_ENV]
    } else {
        &[MAX_ROUNDS_ENV]
    };
    let subagents_keys: &[&str] = if on_subscription_route {
        &[SUBSCRIPTION_MAX_SUBAGENTS_ENV, MAX_SUBAGENTS_ENV]
    } else {
        &[MAX_SUBAGENTS_ENV]
    };
    RunBudget {
        max_rounds: first_usable(
            rounds_keys,
            lookup,
            1,
            MAX_ROUNDS_CEILING,
            DEFAULT_MAX_ROUNDS,
        ),
        max_subagents: first_usable(
            subagents_keys,
            lookup,
            0,
            MAX_SUBAGENTS_CEILING,
            DEFAULT_MAX_SUBAGENTS,
        ),
    }
}

/// [`resolve_run_budget`] for the process environment right now. Read at each
/// dispatch rather than cached, like the other `AGENTIC_*` switches: a deployed
/// process never changes its environment, and a test that sets a value is not
/// at the mercy of which test touched it first.
#[must_use]
pub fn run_budget_from_env(on_subscription_route: bool) -> RunBudget {
    resolve_run_budget(on_subscription_route, &|key| std::env::var(key).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A lookup over a fixed set of variables.
    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        let map: HashMap<&str, &str> = pairs.iter().copied().collect();
        move |key| map.get(key).map(|value| (*value).to_owned())
    }

    fn resolve(on_subscription_route: bool, pairs: &[(&str, &str)]) -> RunBudget {
        resolve_run_budget(on_subscription_route, &env(pairs))
    }

    const TODAYS: RunBudget = RunBudget {
        max_rounds: 4,
        max_subagents: 0,
    };

    #[test]
    fn with_nothing_set_the_budget_is_what_the_dispatch_site_always_sent() {
        assert_eq!(RunBudget::default(), TODAYS);
        assert_eq!(resolve(false, &[]), TODAYS);
        assert_eq!(resolve(true, &[]), TODAYS, "a subscription turn too");
    }

    #[test]
    fn an_empty_or_blank_variable_is_unset() {
        // Compose passes `${VAR:-}` as an empty string.
        let blanks = [
            (MAX_ROUNDS_ENV, ""),
            (MAX_SUBAGENTS_ENV, "  "),
            (SUBSCRIPTION_MAX_ROUNDS_ENV, ""),
            (SUBSCRIPTION_MAX_SUBAGENTS_ENV, "\t"),
        ];
        assert_eq!(resolve(false, &blanks), TODAYS);
        assert_eq!(resolve(true, &blanks), TODAYS);
    }

    #[test]
    fn the_general_variables_apply_to_every_route() {
        let pairs = [(MAX_ROUNDS_ENV, "7"), (MAX_SUBAGENTS_ENV, "3")];
        let expected = RunBudget {
            max_rounds: 7,
            max_subagents: 3,
        };
        assert_eq!(resolve(false, &pairs), expected);
        assert_eq!(resolve(true, &pairs), expected);
    }

    #[test]
    fn a_subscription_route_picks_the_subscription_values() {
        let pairs = [
            (MAX_ROUNDS_ENV, "7"),
            (MAX_SUBAGENTS_ENV, "3"),
            (SUBSCRIPTION_MAX_ROUNDS_ENV, "10"),
            (SUBSCRIPTION_MAX_SUBAGENTS_ENV, "2"),
        ];
        assert_eq!(
            resolve(true, &pairs),
            RunBudget {
                max_rounds: 10,
                max_subagents: 2
            }
        );
    }

    #[test]
    fn the_subscription_variables_are_ignored_off_the_subscription() {
        let pairs = [
            (SUBSCRIPTION_MAX_ROUNDS_ENV, "10"),
            (SUBSCRIPTION_MAX_SUBAGENTS_ENV, "2"),
        ];
        assert_eq!(
            resolve(false, &pairs),
            TODAYS,
            "a turn that is not on the subscription must not see the subscription's budget"
        );
        let with_general = [
            (MAX_ROUNDS_ENV, "6"),
            (SUBSCRIPTION_MAX_ROUNDS_ENV, "10"),
            (SUBSCRIPTION_MAX_SUBAGENTS_ENV, "2"),
        ];
        assert_eq!(
            resolve(false, &with_general),
            RunBudget {
                max_rounds: 6,
                max_subagents: 0
            }
        );
    }

    #[test]
    fn each_knob_falls_back_independently() {
        // Only the rounds are set for the subscription: the subagents come from
        // the general variable.
        let pairs = [(SUBSCRIPTION_MAX_ROUNDS_ENV, "9"), (MAX_SUBAGENTS_ENV, "5")];
        assert_eq!(
            resolve(true, &pairs),
            RunBudget {
                max_rounds: 9,
                max_subagents: 5
            }
        );
    }

    #[test]
    fn an_unusable_subscription_value_falls_through_to_the_general_one() {
        let pairs = [
            (SUBSCRIPTION_MAX_ROUNDS_ENV, "many"),
            (MAX_ROUNDS_ENV, "6"),
            (SUBSCRIPTION_MAX_SUBAGENTS_ENV, "-1"),
            (MAX_SUBAGENTS_ENV, "4"),
        ];
        assert_eq!(
            resolve(true, &pairs),
            RunBudget {
                max_rounds: 6,
                max_subagents: 4
            }
        );
    }

    #[test]
    fn unusable_values_never_widen_or_break_a_run() {
        for bad in ["abc", "-3", "4.5", "1e3", "0x10", "99999999999999999999", "4 rounds"] {
            let pairs = [(MAX_ROUNDS_ENV, bad), (MAX_SUBAGENTS_ENV, bad)];
            assert_eq!(resolve(false, &pairs), TODAYS, "{bad:?}");
            assert_eq!(resolve(true, &pairs), TODAYS, "{bad:?}");
        }
    }

    #[test]
    fn zero_rounds_is_refused_because_execution_core_would_read_it_as_twelve() {
        let pairs = [(MAX_ROUNDS_ENV, "0"), (SUBSCRIPTION_MAX_ROUNDS_ENV, "0")];
        assert_eq!(resolve(false, &pairs).max_rounds, DEFAULT_MAX_ROUNDS);
        assert_eq!(resolve(true, &pairs).max_rounds, DEFAULT_MAX_ROUNDS);
    }

    #[test]
    fn zero_subagents_is_a_value_and_does_not_fall_through() {
        // An explicit 0 on the subscription is the operator saying "execution-
        // core's default", and must not be replaced by the general variable.
        let pairs = [(SUBSCRIPTION_MAX_SUBAGENTS_ENV, "0"), (MAX_SUBAGENTS_ENV, "5")];
        assert_eq!(resolve(true, &pairs).max_subagents, 0);
        assert_eq!(resolve(false, &pairs).max_subagents, 5);
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        let pairs = [(MAX_ROUNDS_ENV, " 8 "), (MAX_SUBAGENTS_ENV, "\n2\n")];
        assert_eq!(
            resolve(false, &pairs),
            RunBudget {
                max_rounds: 8,
                max_subagents: 2
            }
        );
    }

    #[test]
    fn values_are_clamped_to_the_ceilings() {
        let pairs = [(MAX_ROUNDS_ENV, "5000"), (MAX_SUBAGENTS_ENV, "5000")];
        assert_eq!(
            resolve(false, &pairs),
            RunBudget {
                max_rounds: MAX_ROUNDS_CEILING,
                max_subagents: MAX_SUBAGENTS_CEILING
            }
        );
        let at_the_ceiling = [
            (MAX_ROUNDS_ENV, "32"),
            (MAX_SUBAGENTS_ENV, "100"),
        ];
        assert_eq!(
            resolve(false, &at_the_ceiling),
            RunBudget {
                max_rounds: 32,
                max_subagents: 100
            }
        );
    }

    #[test]
    fn the_variable_names_are_the_ones_the_compose_file_passes() {
        // Spelled out so renaming one is a deliberate edit that fails here and
        // not a silent drift from docker-compose.yml and the runbook.
        assert_eq!(MAX_ROUNDS_ENV, "AGENTIC_MAX_ROUNDS");
        assert_eq!(MAX_SUBAGENTS_ENV, "AGENTIC_MAX_SUBAGENTS");
        assert_eq!(SUBSCRIPTION_MAX_ROUNDS_ENV, "AGENTIC_SUBSCRIPTION_MAX_ROUNDS");
        assert_eq!(
            SUBSCRIPTION_MAX_SUBAGENTS_ENV,
            "AGENTIC_SUBSCRIPTION_MAX_SUBAGENTS"
        );
    }
}
