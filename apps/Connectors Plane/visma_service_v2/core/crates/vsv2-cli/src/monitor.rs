//! `vsv2 monitor`: is the pipeline healthy right now?
//!
//! Reads only the state database (Postgres: `scheduler_pass` and `sync_run`) and the tenant list in
//! `config/tenants`. It needs no Visma credentials, no BigQuery and no Redis, and it writes
//! nothing. It evaluates the checks of [`evaluate`], prints the report, optionally posts a
//! summary to a chat webhook, and exits 0 (healthy), 1 (a problem) or 2 (the state could not be
//! read), so a cron job, a systemd timer, a Cloud Scheduler job or a healthcheck can act on it.
//!
//! The command keeps no state: run it every 15 to 30 minutes, and a problem that persists is
//! reported (and posted) by every run until it is fixed.
//!
//! Split in four: [`evaluate`] (rows in, findings out), [`report`] (text, JSON, webhook message and
//! exit code), [`webhook`] (the POST) and [`secret`] (keeping the URLs out of the output). The SQL
//! is in `vsv2_state::monitor`. What the checks are and how to run the command:
//! `docs/DEPLOYMENT.md`, "Monitoring".

mod evaluate;
mod report;
mod secret;
mod webhook;

use std::io::Write;
use std::path::Path;
use std::time::Duration as StdDuration;

use anyhow::{bail, Context};
use chrono::{Duration, Utc};
use clap::{Args, ValueEnum};
use vsv2_common::load_tenants;
use vsv2_orchestrator::ScheduledTenant;
use vsv2_state::monitor::{snapshot_of, Snapshot, SnapshotRequest};

use evaluate::{evaluate, open_set_run_labels, Thresholds};
use report::{Report, EXIT_UNREADABLE};
use secret::{redact, url_secrets, Secret};

/// The environment variable holding the state database URL (the one `schedule` uses).
const DATABASE_URL_VAR: &str = "VSV2_DATABASE_URL";

/// The longest the whole read of the state may take, connecting included.
const READ_TIMEOUT: StdDuration = StdDuration::from_secs(60);

/// Rows read per tenant and tier, and runs per endpoint, beyond the number of failures in a row
/// that make a problem: room for abandoned passes, which the failure count steps over.
const EXTRA_HISTORY: i64 = 20;

/// Open-set runs older than this are not read; a tenant with none newer has none to show.
const RUN_LOOKBACK_DAYS: i32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

/// The options of `vsv2 monitor`.
#[derive(Debug, Clone, Args)]
pub struct MonitorArgs {
    /// How the report is printed on stdout.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,

    /// Directory of the tenant files: the tenants that must be healthy.
    #[arg(long, default_value = "config/tenants")]
    tenants_dir: String,

    /// Incoming webhook (Slack, Teams) to post a summary to when there is a problem. Set it in
    /// the environment rather than on the command line, which shows in the process list. The URL
    /// is never printed.
    #[arg(long, env = "VSV2_MONITOR_WEBHOOK_URL", hide_env_values = true)]
    webhook_url: Option<Secret>,

    /// The newest succeeded hourly pass may be this many minutes old (three times the interval).
    #[arg(
        long,
        default_value_t = evaluate::DEFAULT_HOURLY_MAX_AGE_MINS,
        value_parser = clap::value_parser!(u32).range(1..=525_600)
    )]
    hourly_max_age_mins: u32,

    /// The newest succeeded daily pass may be this many hours old.
    #[arg(
        long,
        default_value_t = evaluate::DEFAULT_DAILY_MAX_AGE_HOURS,
        value_parser = clap::value_parser!(u32).range(1..=8_760)
    )]
    daily_max_age_hours: u32,

    /// The newest succeeded weekly full pass may be this many days old.
    #[arg(
        long,
        default_value_t = evaluate::DEFAULT_FULL_MAX_AGE_DAYS,
        value_parser = clap::value_parser!(u32).range(1..=365)
    )]
    full_max_age_days: u32,

    /// The newest open-set run of an active tenant may be this many hours old.
    #[arg(
        long,
        default_value_t = evaluate::DEFAULT_OPEN_SET_MAX_AGE_HOURS,
        value_parser = clap::value_parser!(u32).range(1..=8_760)
    )]
    open_set_max_age_hours: u32,

    /// A running pass whose heartbeat is this many minutes old is stuck (the scheduler died).
    #[arg(
        long,
        default_value_t = evaluate::DEFAULT_STUCK_HEARTBEAT_MINS,
        value_parser = clap::value_parser!(u32).range(1..=525_600)
    )]
    stuck_heartbeat_mins: u32,

    /// An endpoint that failed in this many finished passes in a row is a problem (fewer: a
    /// warning).
    #[arg(
        long,
        default_value_t = evaluate::DEFAULT_REPEAT_FAILURES,
        value_parser = clap::value_parser!(u32).range(1..=100)
    )]
    repeat_failures: u32,

    /// Exit 1 on a warning too.
    #[arg(long)]
    strict: bool,
}

impl MonitorArgs {
    fn thresholds(&self) -> Thresholds {
        Thresholds {
            hourly_max_age: Duration::minutes(i64::from(self.hourly_max_age_mins)),
            daily_max_age: Duration::hours(i64::from(self.daily_max_age_hours)),
            full_max_age: Duration::days(i64::from(self.full_max_age_days)),
            open_set_max_age: Duration::hours(i64::from(self.open_set_max_age_hours)),
            stuck_heartbeat: Duration::minutes(i64::from(self.stuck_heartbeat_mins)),
            repeat_failures: self.repeat_failures as usize,
        }
    }

    /// The webhook to post to, if one is set and not blank (an empty variable counts as unset).
    fn webhook(&self) -> Option<&Secret> {
        self.webhook_url.as_ref().filter(|url| !url.is_blank())
    }
}

/// Run the monitor and return the exit code for the process: 0, 1 or 2 (see `report::exit_code`).
///
/// The report goes to stdout. What is wrong with the monitor itself (the tenant list, a webhook
/// that did not take the message) goes to stderr, without the URLs.
pub async fn run(args: MonitorArgs) -> i32 {
    let tenants = match load_tenant_list(&args.tenants_dir) {
        Ok(tenants) => tenants,
        Err(error) => {
            eprintln!("vsv2 monitor: {error:#}");
            return EXIT_UNREADABLE;
        }
    };
    let limits = args.thresholds();
    let report = match read_state(&limits).await {
        Ok(snapshot) => {
            let findings = evaluate(&tenants, &snapshot, &limits, snapshot.database_time);
            Report::checked(snapshot.database_time, tenants.len(), findings)
        }
        Err(reason) => Report::unreadable(Utc::now(), tenants.len(), reason),
    };
    print_report(&report, &args);
    alert(&args, &report).await;
    report.exit_code(args.strict)
}

fn load_tenant_list(dir: &str) -> anyhow::Result<Vec<ScheduledTenant>> {
    let configs = load_tenants(Path::new(dir))
        .with_context(|| format!("cannot read the tenant list in {dir}"))?;
    if configs.is_empty() {
        bail!("{dir} has no tenant files, so there is nothing to check");
    }
    Ok(configs.iter().map(ScheduledTenant::from).collect())
}

/// The state of the pipeline, or why it could not be read (the database URL masked).
async fn read_state(limits: &Thresholds) -> Result<Snapshot, String> {
    let dsn = std::env::var(DATABASE_URL_VAR)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{DATABASE_URL_VAR} is not set"))?;
    let labels = open_set_run_labels();
    let depth = limits.repeat_failures as i64 + EXTRA_HISTORY;
    let request = SnapshotRequest {
        passes_per_tier: depth,
        run_endpoints: &labels,
        runs_per_endpoint: depth,
        run_lookback_days: RUN_LOOKBACK_DAYS,
    };
    match tokio::time::timeout(READ_TIMEOUT, snapshot_of(&dsn, &request)).await {
        Ok(Ok(snapshot)) => Ok(snapshot),
        Ok(Err(error)) => Err(redact(&error.to_string(), &url_secrets(&dsn))),
        Err(_) => Err(format!(
            "the read did not finish within {} s",
            READ_TIMEOUT.as_secs()
        )),
    }
}

fn print_report(report: &Report, args: &MonitorArgs) {
    let text = match args.format {
        Format::Text => report.to_text(),
        Format::Json => report.to_json(args.strict),
    };
    let mut out = std::io::stdout().lock();
    if let Err(error) = writeln!(out, "{}", text.trim_end()).and_then(|()| out.flush()) {
        eprintln!("vsv2 monitor: could not write the report: {error}");
    }
}

/// Post the summary when there is a problem and a webhook is set. A delivery that fails is
/// reported on stderr and changes nothing else, least of all the exit code.
async fn alert(args: &MonitorArgs, report: &Report) {
    let (Some(url), Some(text)) = (args.webhook(), report.webhook_text()) else {
        return;
    };
    if let Err(reason) = webhook::post_text(url, &text, webhook::TIMEOUT).await {
        eprintln!("vsv2 monitor: the alert was not delivered: {reason}");
    }
}

#[cfg(test)]
mod tests;
