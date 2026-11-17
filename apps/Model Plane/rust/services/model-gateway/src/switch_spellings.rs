//! Startup report for the default-off switches that now read only their exact value.
//!
//! Each switch below is read exactly by its reader: the literal `true`, or `1` for
//! `ALLOW_INSECURE_DEV_DEFAULTS`. Before that change several of them also took `1`, `on`, `yes`,
//! `enabled`, or `true` in another case or with surrounding whitespace. A live environment that
//! still sets one of those spellings now reads as off, silently.
//!
//! This module changes no reader and no switch value. A switch set to such a spelling stays off,
//! as it does under its exact reader. [`warn_former_spellings`] logs one warning per such switch
//! at startup, so the mismatch shows in the logs instead of only as a missing feature.

use tracing::warn;

use crate::{auth, latency_levers, prompt_context, room_metadata};

/// Each switch this service reads exactly, with the one value that turns it on. Keep each name
/// in step with its reader.
pub const EXACT_SWITCHES: &[(&str, &str)] = &[
    ("MODEL_GATEWAY_AUTH_DEV_BYPASS", "true"),
    ("ALLOW_INSECURE_DEV_DEFAULTS", "1"),
    (auth::SERVICE_FLAG_AI_PII, "true"),
    (auth::SERVICE_FLAG_AI_LAYOUT, "true"),
    ("FINETUNE_ENABLED", "true"),
    (latency_levers::DATED_POST_SECTION_ONLY_ENV, "true"),
    (latency_levers::FINER_REVIEW_BATCHES_ENV, "true"),
    (prompt_context::STABLE_PROMPT_ORDER_ENV, "true"),
    (prompt_context::DEDUP_CONTEXT_HISTORY_ENV, "true"),
    ("MODEL_GATEWAY_EXTRACT_TOOL", "true"),
    (room_metadata::LEGACY_PASSTHROUGH_ENV, "true"),
    ("MODEL_GATEWAY_BROWSER_ATTACH", "true"),
    ("MODEL_GATEWAY_BROWSER_ATTACH_POLICY_ENFORCED", "true"),
    ("MODEL_GATEWAY_BROWSER_POLICY_ASK", "true"),
    ("MODEL_GATEWAY_BROWSER_BUDGET_CLAIM", "true"),
    ("MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS", "true"),
    ("MODEL_GATEWAY_BROWSER_KEY_GRANTS", "true"),
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
    fn the_verified_switches_are_all_listed_once() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in EXACT_SWITCHES {
            assert!(seen.insert(*name), "{name} is listed twice");
        }
        for required in [
            "MODEL_GATEWAY_AUTH_DEV_BYPASS",
            "FINETUNE_ENABLED",
            "MODEL_GATEWAY_DATED_POST_SECTION_ONLY",
            "MODEL_GATEWAY_FINER_REVIEW_BATCHES",
            "MODEL_GATEWAY_BROWSER_ATTACH",
            "MODEL_GATEWAY_BROWSER_POLICY_ASK",
            "MODEL_GATEWAY_BROWSER_BUDGET_CLAIM",
            "MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS",
        ] {
            assert!(seen.contains(required), "{required} is not listed");
        }
    }

    #[test]
    fn a_former_on_spelling_is_reported_for_every_listed_switch() {
        // `1`, `on`, `yes`, `enabled`, `TRUE` and padded `true` are the spellings the old readers
        // took as on. Each one is reported against each switch, except its own exact value.
        for (name, exact_value) in EXACT_SWITCHES {
            for former in ["1", "on", "yes", "enabled", "true", "TRUE", " true "] {
                if former == *exact_value {
                    continue;
                }
                assert_eq!(names(&[(*name, former)]), vec![*name], "{name}={former:?}");
            }
        }
    }

    #[test]
    fn the_exact_value_and_the_off_values_are_not_reported() {
        for (name, exact_value) in EXACT_SWITCHES {
            assert!(
                names(&[(*name, *exact_value)]).is_empty(),
                "{name}={exact_value:?}"
            );
            for off in ["", "0", "false", "off", "no"] {
                assert!(names(&[(*name, off)]).is_empty(), "{name}={off:?}");
            }
        }
        assert!(
            names(&[]).is_empty(),
            "an unset environment reports nothing"
        );
    }

    #[test]
    fn the_insecure_gate_reports_true_and_not_its_own_exact_value() {
        assert!(names(&[("ALLOW_INSECURE_DEV_DEFAULTS", "1")]).is_empty());
        assert_eq!(
            names(&[("ALLOW_INSECURE_DEV_DEFAULTS", "true")]),
            vec!["ALLOW_INSECURE_DEV_DEFAULTS"]
        );
    }

    #[test]
    fn a_report_carries_each_switch_with_its_own_exact_value() {
        let env = [
            ("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1"),
            ("FINETUNE_ENABLED", "on"),
            ("MODEL_GATEWAY_BROWSER_ATTACH", "true"),
        ];
        let reported = former_spellings(|name| {
            env.iter()
                .find(|(set, _)| *set == name)
                .map(|(_, value)| (*value).to_owned())
        });
        assert_eq!(
            reported,
            vec![
                ("MODEL_GATEWAY_AUTH_DEV_BYPASS", "true"),
                ("FINETUNE_ENABLED", "true"),
            ]
        );
    }
}
