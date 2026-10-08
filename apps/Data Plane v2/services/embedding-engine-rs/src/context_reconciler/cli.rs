//! The command line of `context-backfill` (CR-T9a), parsed and checked.
//!
//! The binary is a thin wrapper (`src/bin/context_backfill.rs`): it builds what the service
//! builds and calls [`super::Reconciler`]. What it adds is a command line that makes the unsafe
//! runs impossible to start by accident, and that lives here, in the service's own module tree,
//! so that its rules are tested with the service's tests and not only by running the tool.
//!
//! ```text
//! context-backfill --dry-run [--org-id ORG] [--target-version ctx-v2]
//! context-backfill --org-id ORG (--max-units N | --max-usd USD) [--target-version ctx-v2]
//! ```
//!
//! * `--dry-run` reads counts and prints them with an estimated spend, one JSON line per
//!   organization and a total. It leases and writes nothing, and the output holds no content.
//! * A real run needs an organization and at least one cap. A backfill without a cap is how a
//!   bill is made by accident, so it is refused rather than defaulted.
//! * `--target-version` is a prompt refresh: contexts made with another prompt version are
//!   regenerated under this one. It spends one provider call per unit, so a real run with it
//!   needs `--max-units`, which is the cap.

use std::collections::BTreeMap;

use serde_json::json;

use super::backfill::BackfillReport;
use crate::context::PromptVersion;

pub(crate) const USAGE: &str = "\
context-backfill: fill, verify and repair the contexts and vectors of an organization

USAGE
  context-backfill --dry-run [--org-id ORG] [--target-version ctx-v2]
  context-backfill --org-id ORG (--max-units N | --max-usd USD) [--target-version ctx-v2]

  --org-id ORG            the organization (required unless --dry-run)
  --dry-run               print counts and the estimated spend per organization; change nothing
  --max-units N           lease at most N units in all (required with --target-version)
  --max-usd USD           stop before the estimated generation spend passes USD
  --target-version VER    refresh contexts made with another prompt version (ctx-v1 or ctx-v2)

A real run needs --max-units or --max-usd. Run --dry-run first. The service's own environment is
the tool's (DATABASE_URL, QDRANT_URL, NATS_URL, the event key and the token settings).";

#[derive(Debug, PartialEq)]
pub(crate) struct Args {
    pub org_id: Option<String>,
    pub dry_run: bool,
    pub max_usd: Option<f64>,
    pub max_units: Option<u64>,
    pub target_version: Option<PromptVersion>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Parsed {
    Help,
    Run(Args),
}

fn value(flag: &str, next: Option<String>) -> Result<String, String> {
    next.filter(|value| !value.starts_with("--") || value.len() < 3)
        .ok_or_else(|| format!("{flag} needs a value"))
}

pub(crate) fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut org_id = None;
    let mut dry_run = false;
    let mut max_usd = None;
    let mut max_units = None;
    let mut target_version = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut iter = args.into_iter();
    while let Some(flag) = iter.next() {
        if matches!(
            flag.as_str(),
            "--org-id" | "--dry-run" | "--max-usd" | "--max-units" | "--target-version"
        ) && !seen.insert(flag.clone())
        {
            return Err(format!("{flag} was given twice"));
        }
        match flag.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--dry-run" => dry_run = true,
            "--org-id" => {
                let id = value(&flag, iter.next())?;
                if id.trim().is_empty() || id != id.trim() {
                    return Err("--org-id must be an organization id".to_owned());
                }
                org_id = Some(id);
            }
            "--max-usd" => {
                let text = value(&flag, iter.next())?;
                let usd: f64 = text
                    .parse()
                    .map_err(|_| format!("--max-usd must be a number, got `{text}`"))?;
                if !usd.is_finite() || usd <= 0.0 {
                    return Err(format!("--max-usd must be above zero, got `{text}`"));
                }
                max_usd = Some(usd);
            }
            "--max-units" => {
                let text = value(&flag, iter.next())?;
                let units: u64 = text
                    .parse()
                    .map_err(|_| format!("--max-units must be a whole number, got `{text}`"))?;
                if units == 0 {
                    return Err("--max-units must be at least 1".to_owned());
                }
                max_units = Some(units);
            }
            "--target-version" => {
                let text = value(&flag, iter.next())?;
                target_version = Some(
                    PromptVersion::parse(&text)
                        .map_err(|error| format!("--target-version: {error}"))?,
                );
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    if !dry_run {
        if org_id.is_none() {
            return Err("--org-id is required unless --dry-run".to_owned());
        }
        if max_units.is_none() && max_usd.is_none() {
            return Err(
                "a real run needs a cap: --max-units or --max-usd (run --dry-run first to see what a \
                 run would do and cost)"
                    .to_owned(),
            );
        }
        if target_version.is_some() && max_units.is_none() {
            return Err(
                "--target-version spends one provider call per unit it refreshes, and needs \
                 --max-units as its cap"
                    .to_owned(),
            );
        }
    }
    Ok(Parsed::Run(Args {
        org_id,
        dry_run,
        max_usd,
        max_units,
        target_version,
    }))
}

/// What a run did, as one JSON line. Counts and words from a closed set; no content.
pub(crate) fn render(org_id: &str, report: &BackfillReport, embed_price_known: bool) -> String {
    let leased: BTreeMap<&str, usize> = report.leased.iter().map(|(k, v)| (*k, *v)).collect();
    json!({
        "org_id": org_id,
        "passes": report.passes,
        "units_leased": leased,
        "contexts_written": report.generated,
        "legacy_verified": report.verified,
        "vectors_repaired": report.repaired,
        "unresolved": report.unresolved,
        "documents_announced": report.announced,
        "given_up": report.given_up,
        "stopped": report.stopped,
        "estimated_generation_usd": (report.estimated_llm_usd * 1e6).round() / 1e6,
        "spend_cap_covers": if embed_price_known {
            "generation and embedding"
        } else {
            "generation only (no embedding price was given)"
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(args: &[&str]) -> Result<Parsed, String> {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    fn run(args: &[&str]) -> Args {
        match parse_all(args) {
            Ok(Parsed::Run(args)) => args,
            other => panic!("{args:?} should be a run, got {other:?}"),
        }
    }

    #[test]
    fn a_dry_run_needs_no_organization_and_no_cap() {
        let all = run(&["--dry-run"]);
        assert!(all.dry_run);
        assert_eq!((all.org_id, all.max_units, all.max_usd), (None, None, None));
        let one = run(&[
            "--dry-run",
            "--org-id",
            "org-1",
            "--target-version",
            "ctx-v2",
        ]);
        assert_eq!(one.org_id.as_deref(), Some("org-1"));
        assert_eq!(one.target_version, Some(PromptVersion::V2));
    }

    #[test]
    fn a_real_run_needs_an_organization_and_a_cap() {
        let error = parse_all(&["--max-units", "10"]).expect_err("no organization");
        assert!(error.contains("--org-id"), "{error}");
        let error = parse_all(&["--org-id", "org-1"]).expect_err("no cap");
        assert!(
            error.contains("cap") && error.contains("--dry-run"),
            "{error}"
        );
        let capped = run(&["--org-id", "org-1", "--max-usd", "0.5"]);
        assert_eq!(capped.max_usd, Some(0.5));
        let capped = run(&["--org-id", "org-1", "--max-units", "25", "--max-usd", "1"]);
        assert_eq!((capped.max_units, capped.max_usd), (Some(25), Some(1.0)));
        assert!(!capped.dry_run);
    }

    #[test]
    fn a_prompt_refresh_is_capped_by_units() {
        let error = parse_all(&[
            "--org-id",
            "o",
            "--max-usd",
            "1",
            "--target-version",
            "ctx-v2",
        ])
        .expect_err("a refresh capped only by an estimate");
        assert!(error.contains("--max-units"), "{error}");
        let refresh = run(&[
            "--org-id",
            "o",
            "--max-units",
            "5",
            "--target-version",
            "ctx-v2",
        ]);
        assert_eq!(refresh.target_version, Some(PromptVersion::V2));
        let error = parse_all(&["--dry-run", "--target-version", "ctx-v9"]).expect_err("unknown");
        assert!(error.contains("--target-version"), "{error}");
    }

    #[test]
    fn the_numbers_are_checked() {
        for (flag, value) in [
            ("--max-units", "0"),
            ("--max-units", "-3"),
            ("--max-units", "2.5"),
            ("--max-units", "many"),
            ("--max-usd", "0"),
            ("--max-usd", "-1"),
            ("--max-usd", "NaN"),
            ("--max-usd", "inf"),
            ("--max-usd", "cheap"),
        ] {
            let error = parse_all(&["--org-id", "o", flag, value]).expect_err(value);
            assert!(error.contains(flag), "{flag} {value}: {error}");
        }
    }

    #[test]
    fn a_flag_without_a_value_or_twice_or_unknown_is_refused() {
        assert!(parse_all(&["--org-id"])
            .expect_err("no value")
            .contains("needs a value"));
        assert!(parse_all(&["--org-id", "--dry-run"])
            .expect_err("a flag for a value")
            .contains("needs a value"));
        assert!(parse_all(&["--dry-run", "--dry-run"])
            .expect_err("twice")
            .contains("twice"));
        assert!(parse_all(&["--org-id", "a", "--org-id", "b"])
            .expect_err("twice")
            .contains("twice"));
        assert!(parse_all(&["--dry-run", "--all"])
            .expect_err("unknown")
            .contains("--all"));
        assert!(parse_all(&["--org-id", " ", "--max-units", "1"]).is_err());
        assert_eq!(parse_all(&["--help"]), Ok(Parsed::Help));
        assert_eq!(parse_all(&["-h", "--bogus"]), Ok(Parsed::Help));
    }

    #[test]
    fn the_report_names_what_the_spend_cap_covers_and_holds_no_content() {
        let mut report = BackfillReport {
            passes: 3,
            generated: 4,
            verified: 2,
            stopped: "nothing left to do",
            estimated_llm_usd: 0.000_123_456_789,
            ..BackfillReport::default()
        };
        report.leased.insert("generate", 5);
        let line = render("org-1", &report, false);
        let value: serde_json::Value = serde_json::from_str(&line).expect("JSON");
        assert_eq!(value["org_id"], "org-1");
        assert_eq!(value["units_leased"]["generate"], 5);
        assert_eq!(value["contexts_written"], 4);
        assert_eq!(value["estimated_generation_usd"], 0.000_123);
        assert_eq!(
            value["spend_cap_covers"],
            "generation only (no embedding price was given)"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&render("org-1", &report, true)).unwrap()
                ["spend_cap_covers"],
            "generation and embedding"
        );
    }
}
