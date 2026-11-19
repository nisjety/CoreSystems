//! Startup report for the default-off switch that execution-core reads only as the exact literal
//! `true`.
//!
//! `QUARRY_BROWSER_AGENT_ENABLED` was read leniently before the exact-literal rule: it took `1`,
//! `TRUE` and `yes`, with surrounding whitespace trimmed. A live environment that still sets one of
//! those spellings, or a padded `true`, now reads as off, silently: without a report, the browser
//! agent is missing after the next `compose up` with nothing in the logs to say why.
//!
//! This module changes no reader and no switch value. A switch set to such a spelling stays off, as
//! it does under its exact reader. [`warn_former_spellings`] logs one warning at startup, so the
//! mismatch shows in the logs. The spelling test itself is the shared
//! [`mp_contracts::loop_fold::former_spelling`]; only the name lives here.

use tracing::warn;

/// Each switch this service reads exactly, with the one value that turns it on. Keep each name in
/// step with the reader that looks it up by that name (`QuarryAgentClient::from_env`).
pub const EXACT_SWITCHES: &[(&str, &str)] = &[("QUARRY_BROWSER_AGENT_ENABLED", "true")];

/// The switches in [`EXACT_SWITCHES`] whose value, as `lookup` returns it, is a spelling the exact
/// reader does not take. Each is returned with its exact value, in table order.
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
    use crate::quarry_agent::browser_agent_enabled_from;
    use std::path::Path;

    const BROWSER_AGENT: &str = "QUARRY_BROWSER_AGENT_ENABLED";

    /// Former spellings and their case variants: each reads as off where the switch is exact.
    const FORMER_SPELLINGS: &[&str] = &["1", "TRUE", "True", "yes", "YES", "on", "ON", "enabled"];

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
    fn the_browser_agent_switch_is_listed_once_with_the_exact_literal_true() {
        let listed: Vec<_> = EXACT_SWITCHES
            .iter()
            .filter(|(name, _)| *name == BROWSER_AGENT)
            .copied()
            .collect();
        assert_eq!(listed, vec![(BROWSER_AGENT, "true")]);
    }

    #[test]
    fn each_former_spelling_is_reported_and_the_browser_agent_still_reads_off() {
        for former in [
            "1", "TRUE", "True", "yes", "on", "enabled", " 1 ", " true", "true ",
        ] {
            assert_eq!(
                names(&[(BROWSER_AGENT, former)]),
                vec![BROWSER_AGENT],
                "{former:?}"
            );
            assert!(
                !browser_agent_enabled_from(Some(former)),
                "{former:?} must leave the browser agent off"
            );
        }
    }

    #[test]
    fn the_exact_literal_and_the_off_values_are_not_reported() {
        assert!(browser_agent_enabled_from(Some("true")));
        assert!(names(&[(BROWSER_AGENT, "true")]).is_empty());
        for off in ["", " ", "0", "false", "off", "no"] {
            assert!(names(&[(BROWSER_AGENT, off)]).is_empty(), "{off:?}");
            assert!(!browser_agent_enabled_from(Some(off)), "{off:?}");
        }
        assert!(
            names(&[]).is_empty(),
            "an unset environment reports nothing"
        );
        assert!(!browser_agent_enabled_from(None), "unset is off");
    }

    #[test]
    fn the_compose_default_is_the_exact_literal() {
        // The Model Plane compose file declares the switch once. Its default must be the exact
        // literal: a default of `1` reads as off on every stack that sets no ledger value.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../deploy/docker-compose.yml");
        let compose = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()));
        let marker = format!("${{{BROWSER_AGENT}:-");
        let defaults: Vec<&str> = compose
            .match_indices(marker.as_str())
            .map(|(at, _)| {
                compose[at + marker.len()..]
                    .split('}')
                    .next()
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(
            defaults,
            vec!["true"],
            "{BROWSER_AGENT} must be declared once, defaulting to the exact literal"
        );
    }

    /// Every `.rs` file under `dir`, as (path, text).
    fn rust_sources(dir: &Path, out: &mut Vec<(String, String)>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("cannot read {} ({error})", dir.display()));
        for entry in entries {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                rust_sources(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()));
                out.push((path.display().to_string(), text));
            }
        }
    }

    #[test]
    fn no_source_tells_an_operator_to_set_a_former_spelling() {
        // The off-refusal used to say `=1`, which the exact reader reads as off. Every former
        // spelling is checked, not only `1`.
        let mut sources = Vec::new();
        rust_sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut sources,
        );
        assert!(!sources.is_empty(), "no sources were read");
        for former in FORMER_SPELLINGS {
            let set_it = format!("{BROWSER_AGENT}={former}");
            for (path, text) in &sources {
                assert!(
                    !text.contains(set_it.as_str()),
                    "{path} tells an operator to set {set_it}, which the exact reader reads as off"
                );
            }
        }
    }

    #[test]
    fn every_off_hint_names_the_exact_value() {
        // A hint that only says the switch is "not configured" hides a value that is set but
        // reads as off. Each off hint that names the switch must give the exact value to set.
        let mut sources = Vec::new();
        rust_sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut sources,
        );
        let exact = format!("{BROWSER_AGENT}=true");
        let mut hints = 0;
        for (path, text) in &sources {
            for line in text.lines() {
                if line.contains("unavailable") && line.contains(BROWSER_AGENT) {
                    hints += 1;
                    assert!(
                        line.contains(exact.as_str()),
                        "{path} has an off hint that does not name the exact value: {}",
                        line.trim()
                    );
                }
            }
        }
        // The browser-agent loop, the recipe run's planner and the browser MCP tool each have one.
        assert!(
            hints >= 3,
            "expected the three browser-agent off hints, found {hints}"
        );
    }

    #[test]
    fn the_operator_doc_never_tells_an_operator_to_set_a_former_spelling() {
        // The wiring table names the switch. A line that shows a former spelling as its value
        // must also say that the agent stays off: the doc may warn about a value, never ask for it.
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../docs/core-research/execution-core.md");
        let doc = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()));
        let exact = format!("{BROWSER_AGENT}=true");
        assert!(
            doc.contains(exact.as_str()),
            "{} must name the exact value {exact}",
            path.display()
        );
        for line in doc.lines().filter(|line| line.contains(BROWSER_AGENT)) {
            for former in FORMER_SPELLINGS {
                let set_it = format!("{BROWSER_AGENT}={former}");
                assert!(
                    !line.contains(set_it.as_str()) || line.to_ascii_lowercase().contains("off"),
                    "{} tells an operator to set {set_it} and does not say it stays off",
                    path.display()
                );
            }
        }
    }
}
