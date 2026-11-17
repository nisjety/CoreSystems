//! Startup report for the default-off switches that now read only the literal `true`.
//!
//! Each switch below was read leniently before the exact-literal rule. It took `1`, and `true`
//! in another case or with surrounding whitespace. A live environment that still sets `1` now
//! reads as off, silently. Each one is deny-by-default: off keeps the EU and the residency gates
//! closed.
//!
//! This module changes no reader and no switch value. A switch set to `1` stays off, as it does
//! under its exact reader. [`warn_former_spellings`] logs one warning per such switch at startup,
//! so the mismatch shows in the logs instead of only as a refused deployment or provider.

use tracing::warn;

/// Each switch this service reads exactly, with the one value that turns it on. Keep each name
/// in step with its reader.
pub const EXACT_SWITCHES: &[(&str, &str)] = &[
    ("MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT", "true"),
    ("MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY", "true"),
    ("MODEL_PLANE_ALLOW_NON_EU_EMBEDDING", "true"),
    ("MODEL_PLANE_ALLOW_NON_EU_TTS", "true"),
];

/// The switches in [`EXACT_SWITCHES`] whose value, as `lookup` returns it, is a spelling the
/// exact reader does not take. Each is returned with its exact value, in table order.
#[must_use]
pub fn former_spellings(
    lookup: impl Fn(&str) -> Option<String>,
) -> Vec<(&'static str, &'static str)> {
    EXACT_SWITCHES
        .iter()
        .copied()
        .filter(|&(name, literal)| {
            mp_contracts::loop_fold::former_spelling(literal, lookup(name).as_deref())
        })
        .collect()
}

/// Logs one warning for each switch that is set to a former spelling. Called once, at startup,
/// before either listener opens. It changes no switch: each one stays off.
pub fn warn_former_spellings() {
    for (name, exact_value) in former_spellings(|name| std::env::var(name).ok()) {
        warn!(
            switch = name,
            exact_value,
            "switch is set to a value its exact reader does not take, so it reads as off; \
             set it to exactly this value to turn it on"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names reported for the given `(name, value)` environment.
    fn names(env: &[(&'static str, &'static str)]) -> Vec<&'static str> {
        former_spellings(|name| {
            env.iter()
                .find(|(set, _)| *set == name)
                .map(|(_, value)| (*value).to_owned())
        })
        .into_iter()
        .map(|(name, _)| name)
        .collect()
    }

    #[test]
    fn the_allow_switches_are_all_listed_once() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in EXACT_SWITCHES {
            assert!(seen.insert(*name), "{name} is listed twice");
        }
        for required in [
            "MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT",
            "MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY",
            "MODEL_PLANE_ALLOW_NON_EU_EMBEDDING",
            "MODEL_PLANE_ALLOW_NON_EU_TTS",
        ] {
            assert!(seen.contains(required), "{required} is not listed");
        }
    }

    #[test]
    fn the_former_one_is_reported_and_the_exact_literal_is_not() {
        for (name, exact_value) in EXACT_SWITCHES {
            assert_eq!(names(&[(*name, "1")]), vec![*name], "{name}=1");
            assert!(
                names(&[(*name, *exact_value)]).is_empty(),
                "{name}={exact_value:?}"
            );
        }
    }

    #[test]
    fn the_other_former_spellings_are_reported_and_off_values_are_not() {
        for (name, _) in EXACT_SWITCHES {
            for former in ["on", "yes", "TRUE", " true "] {
                assert_eq!(names(&[(*name, former)]), vec![*name], "{name}={former:?}");
            }
            for off in ["", "0", "false", "off", "no"] {
                assert!(names(&[(*name, off)]).is_empty(), "{name}={off:?}");
            }
        }
        assert!(
            names(&[]).is_empty(),
            "an unset environment reports nothing"
        );
    }
}
