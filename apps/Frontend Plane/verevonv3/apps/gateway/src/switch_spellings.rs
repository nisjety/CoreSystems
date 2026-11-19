//! Startup report for the default-off switches that the gateway reads only as their exact value
//! (V3c-bff-3, V3d-bff-3, V3e-bff-3).
//!
//! Each switch in [`reported_switches`] is on only for the exact literal `true`, and
//! `ALLOW_INSECURE_DEV_DEFAULTS` only for the exact literal `1`. Before those readers were made exact,
//! several of them also took `1`, `yes`, `on` or `true` in another case or with surrounding whitespace.
//! A live environment that still sets one of those spellings now reads as off, silently.
//!
//! This module changes no reader and no switch value. [`warn_former_spellings`] logs one warning per
//! such switch at startup, so the mismatch shows in the logs and not only as a missing route. Each
//! listed switch is read once, by `build_state` in `config.rs`; the source scan in the tests keeps a
//! handler from reading one from the environment per request.

use tracing::warn;

use crate::config::{
    BFF_EXACT_TRUE_SPACE_SWITCHES, BROWSER_EXACT_TRUE_SWITCHES, MODEL_PLANE_EXACT_TRUE_SWITCHES,
};

/// The one switch read only as the exact literal `1`: the second dev gate, which Model Plane reads the
/// same way (`insecure_dev_defaults_from`).
pub(crate) const EXACT_ONE_SWITCHES: [&str; 1] = ["ALLOW_INSECURE_DEV_DEFAULTS"];

/// Every switch this report covers, each with the exact value that turns it on, in table order: the Model
/// Plane mirrors, the BFF-only Space controls and the browser switches (all read as `true`), then the one
/// gate read as `1`.
pub(crate) fn reported_switches() -> Vec<(&'static str, &'static str)> {
    let true_switches = MODEL_PLANE_EXACT_TRUE_SWITCHES
        .iter()
        .chain(BFF_EXACT_TRUE_SPACE_SWITCHES.iter())
        .chain(BROWSER_EXACT_TRUE_SWITCHES.iter())
        .map(|name| (*name, "true"));
    true_switches
        .chain(EXACT_ONE_SWITCHES.iter().map(|name| (*name, "1")))
        .collect()
}

/// Whether `value` is a spelling that a retired lenient reader took as on, and that the exact reader does
/// not take: `1`, `on`, `yes` or `true` in another case or with surrounding whitespace. `literal` is the one
/// value the switch takes. It flags a live value that now reads as off, and it never turns a switch on.
pub(crate) fn is_former_spelling(literal: &str, value: &str) -> bool {
    value != literal
        && matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "on" | "yes" | "true"
        )
}

/// The reported switches whose value, as `lookup` returns it, is a former spelling. Each is returned with
/// its exact value, in table order.
pub(crate) fn former_spellings(
    lookup: impl Fn(&str) -> Option<String>,
) -> Vec<(&'static str, &'static str)> {
    reported_switches()
        .into_iter()
        .filter(|&(name, literal)| {
            lookup(name).is_some_and(|value| is_former_spelling(literal, &value))
        })
        .collect()
}

/// Logs one warning for each listed switch that is set to a former spelling. Called once, at startup,
/// before the listener opens. It changes no switch: each one keeps the reading it had.
pub(crate) fn warn_former_spellings() {
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
    use std::collections::BTreeSet;

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
    fn every_listed_switch_is_listed_once() {
        let mut seen = BTreeSet::new();
        for (name, _) in reported_switches() {
            assert!(seen.insert(name), "{name} is listed twice");
        }
        assert!(
            seen.len() >= 20,
            "the report covers {} switches",
            seen.len()
        );
    }

    #[test]
    fn the_browser_and_space_switches_read_true_and_the_gate_reads_one() {
        for (name, literal) in reported_switches() {
            let expected = if name == "ALLOW_INSECURE_DEV_DEFAULTS" {
                "1"
            } else {
                "true"
            };
            assert_eq!(literal, expected, "{name}");
        }
    }

    #[test]
    fn a_former_spelling_is_reported_for_every_listed_switch_except_its_own_exact_value() {
        for (name, exact_value) in reported_switches() {
            for former in ["1", "on", "yes", "true", "TRUE", "True", " true ", "ON"] {
                let expected: Vec<&str> = if former == exact_value {
                    Vec::new()
                } else {
                    vec![name]
                };
                assert_eq!(names(&[(name, former)]), expected, "{name}={former:?}");
            }
        }
    }

    #[test]
    fn the_exact_value_and_the_off_words_are_not_reported() {
        for (name, exact_value) in reported_switches() {
            assert!(
                names(&[(name, exact_value)]).is_empty(),
                "{name}={exact_value:?}"
            );
            for off in ["", "0", "false", "off", "no", "garbage"] {
                assert!(names(&[(name, off)]).is_empty(), "{name}={off:?}");
            }
        }
        assert!(
            names(&[]).is_empty(),
            "an unset environment reports nothing"
        );
    }

    #[test]
    fn the_gate_reports_true_and_not_its_exact_one() {
        assert!(names(&[("ALLOW_INSECURE_DEV_DEFAULTS", "1")]).is_empty());
        assert_eq!(
            names(&[("ALLOW_INSECURE_DEV_DEFAULTS", "true")]),
            vec!["ALLOW_INSECURE_DEV_DEFAULTS"]
        );
    }

    #[test]
    fn a_report_names_each_switch_with_its_own_exact_value_in_table_order() {
        let reported = former_spellings(|name| match name {
            "SPACE_TIMELINE_API" => Some("1".to_owned()),
            "BROWSER_TAB_MENTIONS" => Some("yes".to_owned()),
            "BROWSER_HUMAN_BROWSE" => Some("true".to_owned()),
            "ALLOW_INSECURE_DEV_DEFAULTS" => Some("on".to_owned()),
            _ => None,
        });
        assert_eq!(
            reported,
            vec![
                ("SPACE_TIMELINE_API", "true"),
                ("BROWSER_TAB_MENTIONS", "true"),
                ("ALLOW_INSECURE_DEV_DEFAULTS", "1"),
            ]
        );
    }

    /// The production source files of the crate, outside `config.rs` and this file.
    fn source_files_outside_config(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("the gateway source directory is readable") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                source_files_outside_config(&path, out);
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let is_rust = path.extension().is_some_and(|ext| ext == "rs");
            if is_rust
                && name != "config.rs"
                && name != "switch_spellings.rs"
                && !name.ends_with("_tests.rs")
            {
                out.push(path);
            }
        }
    }

    /// No handler reads a listed switch from the environment (V3e-bff-3): each is read once, by
    /// `build_state` in `config.rs`, so a per-request read cannot race a parallel test.
    #[test]
    fn no_listed_switch_is_read_from_the_environment_outside_config() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        source_files_outside_config(&src, &mut files);
        assert!(files.len() > 30, "the scan saw {} files", files.len());
        let mut offences = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("a source file is readable");
            for (switch, _) in reported_switches() {
                for read in ["env::var(\"", "env::var_os(\"", "env_exact_true(\""] {
                    if text.contains(&format!("{read}{switch}\"")) {
                        offences.push(format!("{}: {switch}", file.display()));
                    }
                }
            }
        }
        assert!(offences.is_empty(), "{offences:?}");
    }
}
