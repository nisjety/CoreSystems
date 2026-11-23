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
//!
//! The mode switches are read by their words instead (see [`MODE_SWITCHES`]). A set value that is
//! not one of those words reads as the non-enforcing mode, and [`warn_unreadable_modes`] logs one
//! warning for each such switch at startup.

use tracing::warn;

/// Each switch this service reads exactly, with the one value that turns it on. Each name is the
/// reader's own constant, so a renamed reader fails to compile instead of leaving a dead entry.
pub const EXACT_SWITCHES: &[(&str, &str)] = &[
    (crate::audit_publisher::AUTHZ_DECISION_AUDIT_ENV, "true"),
    (crate::audit_publisher::APPROVAL_NOTIFY_ENV, "true"),
    (crate::audit_publisher::SPACE_MEMORY_DELETE_AUDIT_ENV, "true"),
    (crate::orchestration_store::TODO_RECITATION_ENV, "true"),
    (crate::dream_extractor::LLM_EXTRACTION_ENV, "true"),
    (crate::memory_grounding::GROUNDING_ENABLED_ENV, "true"),
    (crate::space::attention_outbox::EMIT_ENV, "true"),
    (crate::space::events::EMIT_ENV, "true"),
    (crate::trace_nats::CONSUMER_ENV, "true"),
    (crate::gdpr::THREAD_MEMORY_ERASURE_ENV, "true"),
    (crate::space_memory::SPACE_MEMORY_ENABLED_ENV, "true"),
    (crate::tool_ledger::ENABLED_ENV, "true"),
    (crate::orchestration_grpc::APPROVAL_AUDIT_EVENTS_ENV, "true"),
    (crate::orchestration_grpc::PROOF_WALKTHROUGH_ENV, "true"),
    (crate::routing_policy_grpc::CANDIDATES_ENV, "true"),
    (crate::routing_policy_grpc::ORG_OVERLAY_ENV, "true"),
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

/// The mode switches this service reads by their words, each with the reader's own test for a set
/// value that the reader does not read exactly. Such a value reads as the non-enforcing mode (report,
/// or observe), or as off when it is only padded `off`, so it never turns enforcement on. Each name
/// and test is the reader's own, so a renamed reader fails to compile instead of leaving a dead entry.
pub const MODE_SWITCHES: &[(&str, fn(&str) -> bool)] = &[
    (
        crate::write_policy::MODE_ENV,
        crate::write_policy::is_unreadable_mode,
    ),
    (
        mp_contracts::skill_lint::MODE_ENV,
        mp_contracts::skill_lint::is_unreadable_mode,
    ),
    (
        mp_contracts::retention::SPACE_ZDR_ENV,
        mp_contracts::retention::space_zdr_mode_is_unreadable,
    ),
    (
        crate::retention_sweeper::SWITCH_ENV,
        crate::retention_sweeper::is_unreadable_mode,
    ),
];

/// The names of the [`MODE_SWITCHES`] whose value, as `lookup` returns it, is set to a value its
/// reader does not read.
#[must_use]
pub fn unreadable_modes(lookup: impl Fn(&str) -> Option<String>) -> Vec<&'static str> {
    MODE_SWITCHES
        .iter()
        .filter(|(name, unreadable)| lookup(*name).is_some_and(|value| unreadable(value.as_str())))
        .map(|(name, _)| *name)
        .collect()
}

/// Logs one warning at startup for each mode switch set to a value its reader does not read. It
/// names the switch and never the value, and it changes nothing.
pub fn warn_unreadable_modes() {
    warn_unreadable_from(|name| std::env::var(name).ok());
}

/// [`warn_unreadable_modes`] over any environment `lookup` reads.
fn warn_unreadable_from(lookup: impl Fn(&str) -> Option<String>) {
    for name in unreadable_modes(lookup) {
        warn!(
            switch = name,
            "switch is set to a value that is not one of its words, so it reads as the non-enforcing \
             mode; set it to one of its words, or unset it. The value is not logged"
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

    /// The mode switches named for `value` set on one switch, and nothing else.
    fn flagged(name: &str, value: &str) -> Vec<&'static str> {
        unreadable_modes(|set| (set == name).then(|| value.to_owned()))
    }

    #[test]
    fn a_mode_switch_is_named_for_a_set_value_its_reader_does_not_read_and_not_for_its_words() {
        for name in [
            crate::write_policy::MODE_ENV,
            mp_contracts::skill_lint::MODE_ENV,
        ] {
            for word in ["", "   ", "off", "report", "enforce"] {
                assert!(flagged(name, word).is_empty(), "{name}={word:?}");
            }
            for junk in [
                " report",
                "report\r\n",
                "enforce\r\n",
                "ENFORCE",
                "enforced",
                "true",
                "maybe",
                " off ",
            ] {
                assert_eq!(flagged(name, junk), vec![name], "{name}={junk:?}");
            }
        }
        let zdr = mp_contracts::retention::SPACE_ZDR_ENV;
        for word in ["", "   ", "enforce", " ENFORCE\r\n", "observe", " Observe "] {
            assert!(flagged(zdr, word).is_empty(), "{zdr}={word:?}");
        }
        for junk in ["true", "1", "on", "enforced", "enforce me", "off"] {
            assert_eq!(flagged(zdr, junk), vec![zdr], "{zdr}={junk:?}");
        }
    }

    #[test]
    fn an_unset_mode_switch_is_not_named() {
        assert!(unreadable_modes(|_| None).is_empty());
    }

    /// `SESSION_RETENTION_SWEEP` is named the way the other mode switches are: a padded or cased
    /// `enforce` reads as dry run and is reported, while its words and the off values are not.
    #[test]
    fn the_retention_sweep_switch_names_a_padded_or_cased_enforce() {
        let name = crate::retention_sweeper::SWITCH_ENV;
        for word in ["", "   ", "off", " OFF ", "dry_run", "enforce"] {
            assert!(flagged(name, word).is_empty(), "{name}={word:?}");
        }
        for junk in [" enforce ", "Enforce", "enforce\r\n", "ENFORCE", "true"] {
            assert_eq!(flagged(name, junk), vec![name], "{name}={junk:?}");
        }
    }

    /// The startup report writes one warning per unreadable mode switch, naming the switch, and
    /// never the value the switch was set to. A padded, cased `enforce` is unreadable too: it reads
    /// as report, and the report names it.
    #[test]
    fn the_startup_report_names_each_unreadable_mode_switch_and_never_its_value() {
        let lines = crate::test_log_capture::CapturedLines::default();
        tracing::subscriber::with_default(lines.clone(), || {
            warn_unreadable_from(|set| {
                [
                    (crate::write_policy::MODE_ENV, "maybe-a-junk-word"),
                    (mp_contracts::skill_lint::MODE_ENV, " ENFORCE\r\n"),
                    (mp_contracts::retention::SPACE_ZDR_ENV, "true"),
                ]
                .into_iter()
                .find(|(name, _)| *name == set)
                .map(|(_, value)| value.to_owned())
            });
        });
        let logged = lines.lines();
        assert_eq!(logged.len(), 3, "{logged:?}");
        let named = [
            crate::write_policy::MODE_ENV,
            mp_contracts::skill_lint::MODE_ENV,
            mp_contracts::retention::SPACE_ZDR_ENV,
        ];
        for (line, name) in logged.iter().zip(named) {
            assert!(
                line.starts_with("WARN ") && line.contains(&format!("switch=\"{name}\"")),
                "{logged:?}"
            );
        }
        for line in &logged {
            assert!(!line.contains("maybe-a-junk-word"), "{line}");
            assert!(!line.contains("ENFORCE"), "{line}");
            assert!(!line.contains("true"), "{line}");
        }
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
