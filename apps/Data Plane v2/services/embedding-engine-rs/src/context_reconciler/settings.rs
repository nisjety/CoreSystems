//! The reconciler's settings, read from the service's configuration and checked at startup.
//!
//! Every switch the reconciler adds has a default, a check here, and a line in the runbook.
//! A value out of range stops the service and names the variable; nothing is clamped silently,
//! because a limit that is quietly lowered is a limit nobody knows about.

use std::fmt::Display;
use std::path::PathBuf;
use std::time::Duration;

use crate::config::Config;
use crate::context::InlineBudget;

/// What the reconciler does, and how fast.
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    /// Between two passes that found nothing to do.
    pub interval: Duration,
    /// The most units one pass leases, across every organization.
    pub units_per_pass: usize,
    /// The most units one pass leases for one organization.
    pub org_units_per_pass: usize,
    /// Documents worked on at the same time, across every organization.
    pub concurrency: usize,
    /// Documents of one organization worked on at the same time.
    pub org_concurrency: usize,
    /// How long a leased unit belongs to the reconciler that leased it.
    pub lease: Duration,
    /// Failed attempts after which a unit is `failed_permanent`.
    pub max_attempts: i32,
    /// Consecutive failed passes before the breaker opens.
    pub breaker_failures: u32,
    /// How long it stays open.
    pub breaker_pause: Duration,
    /// A file whose existence pauses the reconciler.
    pub pause_file: Option<PathBuf>,
    /// Between two checks of an organization whose units were skipped for zero retention.
    pub zdr_recheck: Duration,
    /// Between two sweeps for the work the work queue index does not cover.
    pub sweep: Duration,
    /// Documents one pass checks for staleness, or `None` when the stale pass is off.
    pub stale_documents: Option<usize>,
    /// The most units this process refreshes in all, or `None` when the refresh is off.
    pub refresh_cap: Option<u64>,
    /// The most document text a prompt carries (`CONTEXTUAL_RETRIEVAL_MAX_DOCUMENT_CHARS`): the
    /// bound a spend estimate has to hold, whatever the window is set to.
    pub prompt_chars_cap: i64,
}

/// The shortest lease: twice the longest time box a batch can have (30 s), so a lease cannot end
/// while the batch that holds it is still running.
const MIN_LEASE_SECS: u64 = 60;

fn within<T: PartialOrd + Display + Copy>(
    variable: &str,
    value: T,
    min: T,
    max: T,
) -> anyhow::Result<T> {
    if value < min || value > max {
        anyhow::bail!("{variable} is {value} but must be between {min} and {max}");
    }
    Ok(value)
}

impl Settings {
    pub(crate) fn from_config(cfg: &Config) -> anyhow::Result<Self> {
        let interval = within(
            "CONTEXT_RECONCILER_INTERVAL_SECS",
            cfg.context_reconciler_interval_secs,
            1,
            3_600,
        )?;
        let units_per_pass = within(
            "CONTEXT_RECONCILER_UNITS_PER_PASS",
            cfg.context_reconciler_units_per_pass,
            1,
            500,
        )?;
        let org_units_per_pass = within(
            "CONTEXT_RECONCILER_ORG_UNITS_PER_PASS",
            cfg.context_reconciler_org_units_per_pass,
            1,
            units_per_pass,
        )?;
        let concurrency = within(
            "CONTEXT_RECONCILER_CONCURRENCY",
            cfg.context_reconciler_concurrency,
            1,
            16,
        )?;
        let org_concurrency = within(
            "CONTEXT_RECONCILER_ORG_CONCURRENCY",
            cfg.context_reconciler_org_concurrency,
            1,
            concurrency,
        )?;
        within(
            "CONTEXT_RECONCILER_CHUNK_CONCURRENCY",
            cfg.context_reconciler_chunk_concurrency,
            1,
            16,
        )?;
        let lease = within(
            "CONTEXT_RECONCILER_LEASE_SECS",
            cfg.context_reconciler_lease_secs,
            MIN_LEASE_SECS,
            3_600,
        )?;
        // The time box is validated by the type the inline path uses, so the bound (30 s) and the
        // message are the same ones.
        InlineBudget::new(
            cfg.context_reconciler_document_budget_ms,
            cfg.context_reconciler_budget_ms,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "{}",
                error
                    .replace("CONTEXT_INLINE_BUDGET_MS", "CONTEXT_RECONCILER_BUDGET_MS")
                    .replace(
                        "CONTEXT_INLINE_DOCUMENT_BUDGET_MS",
                        "CONTEXT_RECONCILER_DOCUMENT_BUDGET_MS"
                    )
            )
        })?;
        let max_attempts = within(
            "CONTEXT_RECONCILER_MAX_ATTEMPTS",
            cfg.context_reconciler_max_attempts,
            1,
            100,
        )?;
        let breaker_failures = within(
            "CONTEXT_RECONCILER_BREAKER_FAILURES",
            cfg.context_reconciler_breaker_failures,
            1,
            20,
        )?;
        let breaker_pause = within(
            "CONTEXT_RECONCILER_BREAKER_PAUSE_SECS",
            cfg.context_reconciler_breaker_pause_secs,
            10,
            3_600,
        )?;
        let zdr_recheck = within(
            "CONTEXT_RECONCILER_ZDR_RECHECK_SECS",
            cfg.context_reconciler_zdr_recheck_secs,
            60,
            86_400,
        )?;
        let sweep = within(
            "CONTEXT_RECONCILER_SWEEP_SECS",
            cfg.context_reconciler_sweep_secs,
            10,
            86_400,
        )?;
        let stale_documents = if cfg.context_reconciler_stale_enabled {
            Some(within(
                "CONTEXT_RECONCILER_STALE_DOCUMENTS",
                cfg.context_reconciler_stale_documents,
                1,
                100,
            )?)
        } else {
            None
        };
        let refresh_cap = if cfg.context_refresh_on_prompt_change {
            if cfg.context_refresh_max_units == 0 {
                anyhow::bail!(
                    "CONTEXT_REFRESH_ON_PROMPT_CHANGE=1 needs CONTEXT_REFRESH_MAX_UNITS above zero: \
                     a prompt refresh is a rollout that spends one provider call per unit, and it \
                     always runs inside an explicit cap"
                );
            }
            Some(cfg.context_refresh_max_units)
        } else {
            None
        };
        let pause_file = {
            let path = cfg.context_reconciler_pause_file.trim();
            (!path.is_empty()).then(|| PathBuf::from(path))
        };
        Ok(Self {
            interval: Duration::from_secs(interval),
            units_per_pass,
            org_units_per_pass,
            concurrency,
            org_concurrency,
            lease: Duration::from_secs(lease),
            max_attempts,
            breaker_failures,
            breaker_pause: Duration::from_secs(breaker_pause),
            pause_file,
            zdr_recheck: Duration::from_secs(zdr_recheck),
            sweep: Duration::from_secs(sweep),
            stale_documents,
            refresh_cap,
            prompt_chars_cap: i64::try_from(cfg.contextual_retrieval_max_document_chars)
                .unwrap_or(i64::MAX),
        })
    }
}

/// The configuration the reconciler's own contextualizer is built from: the same model,
/// provider, residency floor, prompt and window as the inline path (a context is a context,
/// whoever asked for it), with the reconciler's own time box and concurrency, because it has no
/// broker waiting on it and has to leave chat its share of the deployment.
pub(crate) fn contextualizer_config(cfg: &Config) -> Config {
    Config {
        context_inline_budget_ms: cfg.context_reconciler_budget_ms,
        context_inline_document_budget_ms: cfg.context_reconciler_document_budget_ms,
        context_inline_document_concurrency: 1,
        contextual_retrieval_concurrency: cfg.context_reconciler_chunk_concurrency,
        ..cfg.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(extra: &[(&str, &str)]) -> Config {
        let mut vars = vec![
            ("DATABASE_URL".to_owned(), "postgres://x".to_owned()),
            ("NATS_URL".to_owned(), "nats://x".to_owned()),
            ("QDRANT_URL".to_owned(), "http://x".to_owned()),
        ];
        vars.extend(
            extra
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
        );
        envy::from_iter(vars).expect("config")
    }

    fn settings(extra: &[(&str, &str)]) -> anyhow::Result<Settings> {
        Settings::from_config(&config(extra))
    }

    /// The defaults are the ones the compose file and the runbook state, and they are valid.
    #[test]
    fn the_defaults_are_valid_and_conservative() {
        let cfg = config(&[]);
        assert!(!cfg.context_reconciler_enabled, "dark until it is proven");
        assert!(!cfg.context_reconciler_stale_enabled);
        assert!(!cfg.context_refresh_on_prompt_change);
        let s = Settings::from_config(&cfg).expect("defaults");
        assert_eq!(s.interval, Duration::from_secs(30));
        assert_eq!((s.units_per_pass, s.org_units_per_pass), (32, 16));
        assert_eq!((s.concurrency, s.org_concurrency), (2, 1));
        assert_eq!(s.lease, Duration::from_secs(300));
        assert_eq!(s.max_attempts, 8);
        assert_eq!(s.breaker_failures, 3);
        assert_eq!(s.breaker_pause, Duration::from_secs(300));
        assert_eq!(s.zdr_recheck, Duration::from_secs(3_600), "at most hourly");
        assert_eq!(s.sweep, Duration::from_secs(300));
        assert_eq!(
            s.pause_file.as_deref(),
            Some(std::path::Path::new("/tmp/context-reconciler.pause"))
        );
        assert_eq!(s.stale_documents, None);
        assert_eq!(s.refresh_cap, None);
        // The most the reconciler has in flight at the provider is what the inline path's own
        // default is: documents at a time times chunks of each at a time.
        assert_eq!(s.concurrency * cfg.context_reconciler_chunk_concurrency, 4);
    }

    #[test]
    fn a_value_out_of_range_stops_the_service_and_names_the_variable() {
        let cases: &[(&str, &str)] = &[
            ("CONTEXT_RECONCILER_INTERVAL_SECS", "0"),
            ("CONTEXT_RECONCILER_INTERVAL_SECS", "3601"),
            ("CONTEXT_RECONCILER_UNITS_PER_PASS", "0"),
            ("CONTEXT_RECONCILER_UNITS_PER_PASS", "501"),
            ("CONTEXT_RECONCILER_ORG_UNITS_PER_PASS", "33"),
            ("CONTEXT_RECONCILER_CONCURRENCY", "17"),
            ("CONTEXT_RECONCILER_ORG_CONCURRENCY", "3"),
            ("CONTEXT_RECONCILER_CHUNK_CONCURRENCY", "0"),
            ("CONTEXT_RECONCILER_LEASE_SECS", "59"),
            ("CONTEXT_RECONCILER_MAX_ATTEMPTS", "0"),
            ("CONTEXT_RECONCILER_BREAKER_FAILURES", "21"),
            ("CONTEXT_RECONCILER_BREAKER_PAUSE_SECS", "9"),
            ("CONTEXT_RECONCILER_ZDR_RECHECK_SECS", "59"),
            ("CONTEXT_RECONCILER_SWEEP_SECS", "9"),
        ];
        for (variable, value) in cases {
            let error = settings(&[(variable, value)])
                .expect_err(&format!("{variable}={value} must be refused"))
                .to_string();
            assert!(error.contains(variable), "{variable}={value}: {error}");
        }
        // The stale pass is only checked when it is on.
        assert!(settings(&[("CONTEXT_RECONCILER_STALE_DOCUMENTS", "0")]).is_ok());
        let error = settings(&[
            ("CONTEXT_RECONCILER_STALE_ENABLED", "1"),
            ("CONTEXT_RECONCILER_STALE_DOCUMENTS", "101"),
        ])
        .expect_err("101 documents")
        .to_string();
        assert!(
            error.contains("CONTEXT_RECONCILER_STALE_DOCUMENTS"),
            "{error}"
        );
    }

    #[test]
    fn the_time_box_has_the_inline_bound_and_names_the_reconcilers_variables() {
        let error = settings(&[("CONTEXT_RECONCILER_BUDGET_MS", "30001")])
            .expect_err("more than half the ack wait")
            .to_string();
        assert!(error.contains("CONTEXT_RECONCILER_BUDGET_MS"), "{error}");
        assert!(!error.contains("CONTEXT_INLINE"), "{error}");
        let error = settings(&[("CONTEXT_RECONCILER_DOCUMENT_BUDGET_MS", "31000")])
            .expect_err("a document longer than the batch")
            .to_string();
        assert!(
            error.contains("CONTEXT_RECONCILER_DOCUMENT_BUDGET_MS"),
            "{error}"
        );
    }

    /// A lease that ends while its batch is still running hands the unit to a second
    /// reconciler, which pays for the same context twice. The shortest lease the settings accept
    /// is twice the longest time box a batch can have, so the one cannot be set below the other.
    #[test]
    fn the_shortest_lease_outlasts_the_longest_time_box_twice() {
        assert!(
            MIN_LEASE_SECS * 1_000
                >= 2 * crate::context::INLINE_BATCH_BUDGET_CEILING.as_millis() as u64
        );
        let error = settings(&[("CONTEXT_RECONCILER_LEASE_SECS", "59")])
            .expect_err("under the minimum")
            .to_string();
        assert!(error.contains("CONTEXT_RECONCILER_LEASE_SECS"), "{error}");
        assert!(settings(&[("CONTEXT_RECONCILER_LEASE_SECS", "60")]).is_ok());
    }

    /// A refresh is a rollout that spends one provider call per unit: switching it on without a
    /// cap is refused, and the cap is what the settings carry.
    #[test]
    fn a_prompt_refresh_needs_an_explicit_cap() {
        assert_eq!(
            settings(&[]).expect("off").refresh_cap,
            None,
            "off unless the flag is set"
        );
        // A cap without the flag does not switch anything on.
        assert_eq!(
            settings(&[("CONTEXT_REFRESH_MAX_UNITS", "50")])
                .expect("a cap alone")
                .refresh_cap,
            None
        );
        let error = settings(&[("CONTEXT_REFRESH_ON_PROMPT_CHANGE", "1")])
            .expect_err("a refresh without a cap")
            .to_string();
        assert!(error.contains("CONTEXT_REFRESH_MAX_UNITS"), "{error}");
        assert_eq!(
            settings(&[
                ("CONTEXT_REFRESH_ON_PROMPT_CHANGE", "true"),
                ("CONTEXT_REFRESH_MAX_UNITS", "50"),
            ])
            .expect("capped")
            .refresh_cap,
            Some(50)
        );
    }

    #[test]
    fn an_empty_pause_file_disables_the_file_switch() {
        assert_eq!(
            settings(&[("CONTEXT_RECONCILER_PAUSE_FILE", "")])
                .expect("empty")
                .pause_file,
            None
        );
        assert_eq!(
            settings(&[("CONTEXT_RECONCILER_PAUSE_FILE", "  /run/pause  ")])
                .expect("padded")
                .pause_file
                .as_deref(),
            Some(std::path::Path::new("/run/pause"))
        );
    }

    /// The compose file is the control surface an operator really has (it passes the variables the
    /// container sees), so it names every variable the configuration reads for the reconciler,
    /// names no other, and passes the default the code has: a default that differed would be a
    /// setting that changes the day somebody sets the variable to what the runbook says it is.
    #[test]
    fn the_compose_file_names_every_switch_with_the_codes_default() {
        let compose = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docker-compose.yml"
        ))
        .expect("the Data Plane compose file, two directories above the crate")
        .replace('\r', "");
        // The embedding-engine service: from its key to the next key at the same indentation.
        let start = compose
            .find("\n  embedding-engine:\n")
            .expect("the embedding-engine service")
            + 1;
        let mut block = String::new();
        for line in compose[start..].lines() {
            let top_level_key = line.starts_with("  ")
                && !line.starts_with("   ")
                && line.trim_end().ends_with(':')
                && !block.is_empty()
                && !line.starts_with("  embedding-engine:");
            if top_level_key {
                break;
            }
            block.push_str(line);
            block.push('\n');
        }

        // Every variable of the block that belongs to the reconciler, with the default it passes.
        let mut in_compose = std::collections::BTreeMap::new();
        for line in block.lines() {
            let line = line.trim();
            let Some((name, rest)) = line.split_once(": ") else {
                continue;
            };
            if !(name.starts_with("CONTEXT_RECONCILER_") || name.starts_with("CONTEXT_REFRESH_")) {
                continue;
            }
            let (variable, default) = rest
                .strip_prefix("${")
                .and_then(|rest| rest.strip_suffix('}'))
                .and_then(|rest| rest.split_once(":-"))
                .unwrap_or_else(|| panic!("{name} is not passed as a defaulted variable: {rest}"));
            assert_eq!(variable, name, "{name} passes another variable's value");
            in_compose.insert(name.to_owned(), default.to_owned());
        }

        // Every variable the configuration reads for it.
        let config_source = include_str!("../config.rs").replace('\r', "");
        let read: std::collections::BTreeSet<String> = config_source
            .lines()
            .filter_map(|line| line.trim().strip_prefix("pub "))
            .filter_map(|field| field.split_once(':').map(|(name, _)| name.to_owned()))
            .filter(|name| {
                name.starts_with("context_reconciler_") || name.starts_with("context_refresh_")
            })
            .map(|name| name.to_uppercase())
            .collect();
        assert!(
            read.len() >= 20,
            "the configuration lost its reconciler settings: {read:?}"
        );
        let named: std::collections::BTreeSet<String> = in_compose.keys().cloned().collect();
        assert_eq!(
            named, read,
            "the compose file and the configuration must name the same variables"
        );

        // And the defaults it passes are the defaults the code has.
        let base = vec![
            ("DATABASE_URL".to_owned(), "postgres://x".to_owned()),
            ("NATS_URL".to_owned(), "nats://x".to_owned()),
            ("QDRANT_URL".to_owned(), "http://x".to_owned()),
        ];
        let from_compose: Config =
            envy::from_iter(base.iter().cloned().chain(in_compose)).expect("compose defaults");
        let from_code: Config = envy::from_iter(base).expect("code defaults");
        assert_eq!(
            format!("{from_compose:?}"),
            format!("{from_code:?}"),
            "a default in the compose file differs from the one in the code"
        );
    }

    #[test]
    fn the_reconcilers_contextualizer_has_its_own_time_box_and_concurrency_and_nothing_else_changes(
    ) {
        let cfg = config(&[
            ("CONTEXTUAL_RETRIEVAL_ENABLED", "true"),
            ("CONTEXTUAL_RETRIEVAL_MODEL", "gpt-4o-mini"),
            ("CONTEXTUAL_RETRIEVAL_CONCURRENCY", "6"),
            ("CONTEXT_INLINE_BUDGET_MS", "20000"),
            ("CONTEXT_RECONCILER_BUDGET_MS", "28000"),
            ("CONTEXT_RECONCILER_DOCUMENT_BUDGET_MS", "21000"),
            ("CONTEXT_RECONCILER_CHUNK_CONCURRENCY", "3"),
        ]);
        let own = contextualizer_config(&cfg);
        assert_eq!(own.context_inline_budget_ms, 28_000);
        assert_eq!(own.context_inline_document_budget_ms, 21_000);
        assert_eq!(own.contextual_retrieval_concurrency, 3);
        assert_eq!(own.context_inline_document_concurrency, 1);
        // What decides what a context is stays the service's.
        assert_eq!(
            own.contextual_retrieval_model,
            cfg.contextual_retrieval_model
        );
        assert_eq!(
            own.contextual_retrieval_min_residency,
            cfg.contextual_retrieval_min_residency
        );
        assert_eq!(
            own.contextual_retrieval_prompt_version,
            cfg.contextual_retrieval_prompt_version
        );
        assert_eq!(
            own.contextual_retrieval_window_chars,
            cfg.contextual_retrieval_window_chars
        );
        assert!(own.contextual_retrieval_enabled);
        // And the service's own figures were not touched.
        assert_eq!(cfg.context_inline_budget_ms, 20_000);
        assert_eq!(cfg.contextual_retrieval_concurrency, 6);
    }
}
