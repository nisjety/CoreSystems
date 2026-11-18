//! Startup report for the default-off switches that now read only their exact value.
//!
//! Each switch below is read exactly by its reader: the literal `true`. Before that rule several
//! of them also took `1`, `on`, `yes`, or `true` in another case or with surrounding whitespace,
//! and `DREAMING_LLM_EXTRACTION` was on when unset. A live environment that still sets one of
//! those spellings now reads as off, silently.
//!
//! This module changes no reader and no switch value. A switch set to such a spelling stays off,
//! as it does under its exact reader. [`warn_former_spellings`] logs one warning per such switch
//! at startup, so the mismatch shows in the logs instead of only as a missing feature. The spelling
//! test itself is the shared [`mp_contracts::loop_fold::former_spelling`]; only the names live here.

use tracing::warn;

/// Each switch this service reads exactly, with the one value that turns it on. Keep each name
/// in step with its reader.
pub const EXACT_SWITCHES: &[(&str, &str)] = &[
    ("SESSION_CORE_AUTHZ_DECISION_AUDIT", "true"),
    ("SESSION_CORE_APPROVAL_NOTIFY", "true"),
    ("DREAMING_LLM_EXTRACTION", "true"),
    ("MEMORY_GROUNDING_ENABLED", "true"),
    ("SPACE_ATTENTION_EMIT", "true"),
    ("SPACE_REALTIME_EMIT", "true"),
    ("SESSION_CORE_TRACE_CONSUMER", "true"),
    ("SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE", "true"),
    ("SESSION_SPACE_MEMORY_ENABLED", "true"),
    ("SESSION_CORE_TOOL_LEDGER_SWEEP_ENABLED", "true"),
    ("APPROVAL_AUDIT_EVENTS_ENABLED", "true"),
    ("SESSION_PROOF_WALKTHROUGH_ENABLED", "true"),
    ("SESSION_CORE_ROUTING_CANDIDATES_ENABLED", "true"),
    ("SESSION_ORG_ROUTING_OVERLAY", "true"),
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
/// before any listener or background worker starts. It changes no switch: each one stays off.
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
    fn every_listed_switch_is_listed_once_and_the_known_ones_are_present() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in EXACT_SWITCHES {
            assert!(seen.insert(*name), "{name} is listed twice");
        }
        for required in [
            "SESSION_CORE_AUTHZ_DECISION_AUDIT",
            "SESSION_CORE_APPROVAL_NOTIFY",
            "DREAMING_LLM_EXTRACTION",
            "MEMORY_GROUNDING_ENABLED",
            "SPACE_ATTENTION_EMIT",
            "SPACE_REALTIME_EMIT",
            "SESSION_CORE_TRACE_CONSUMER",
        ] {
            assert!(seen.contains(required), "{required} is not listed");
        }
    }

    #[test]
    fn a_former_spelling_is_reported_for_every_listed_switch() {
        // `1`, `on`, `yes`, `enabled`, `TRUE` and padded `true` are spellings an older reader took
        // as on, or that now read as off. Each is reported for each switch, except its own exact
        // value.
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
            for off in ["", " ", "0", "false", "off", "no"] {
                assert!(names(&[(*name, off)]).is_empty(), "{name}={off:?}");
            }
        }
        assert!(
            names(&[]).is_empty(),
            "an unset environment reports nothing"
        );
    }

    #[test]
    fn an_unrecognised_value_is_not_reported_as_a_former_spelling() {
        assert!(names(&[("SPACE_REALTIME_EMIT", "maybe")]).is_empty());
    }

    #[test]
    fn a_report_carries_each_switch_with_its_own_exact_value() {
        let env = [
            ("SPACE_ATTENTION_EMIT", "1"),
            ("DREAMING_LLM_EXTRACTION", "on"),
            ("SESSION_ORG_ROUTING_OVERLAY", "true"),
        ];
        let reported = former_spellings(|name| {
            env.iter()
                .find(|(set, _)| *set == name)
                .map(|(_, value)| (*value).to_owned())
        });
        assert_eq!(
            reported,
            vec![
                ("DREAMING_LLM_EXTRACTION", "true"),
                ("SPACE_ATTENTION_EMIT", "true"),
            ]
        );
    }
}
