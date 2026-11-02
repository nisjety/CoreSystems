//! The health checks: rows in, findings out. No I/O and no clock of its own: `now` is passed in, so
//! every check is a function of the rows and of the time it is given.
//!
//! Per tenant of `config/tenants` (a `historical` tenant only runs the weekly full pass, so only
//! that tier is checked for it, and it has no open-set run):
//!
//! 1. `freshness_<tier>`: the newest *succeeded* pass of the tier is younger than its limit;
//! 2. `stuck_pass`: no pass is `running` with a heartbeat older than the stuck limit (the
//!    scheduler died mid-pass);
//! 3. `failures_<tier>`: the newest finished pass of the tier did not fail; an endpoint that
//!    failed in the last `repeat_failures` finished passes is a problem, a single failure a warning;
//! 4. `open_set_<endpoint>`: the newest open-set run is young enough and did not fail;
//! 5. `tenant_has_run`: the tenant has at least one pass at all.
//!
//! A database without a single pass gives one finding, `scheduler_ran`, instead of one per tenant.

mod failures;
mod freshness;
mod open_set;
mod stuck;
mod words;

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use vsv2_orchestrator::open_set::run_label;
use vsv2_orchestrator::{ScheduledTenant, Tier};
use vsv2_state::monitor::{PassRow, RunRow, Snapshot};

use super::report::{Finding, Severity};
use failures::failures;
use freshness::freshness;
use open_set::open_set;
use stuck::{stuck, stuck_unlisted};

pub const DEFAULT_HOURLY_MAX_AGE_MINS: u32 = 180;
pub const DEFAULT_DAILY_MAX_AGE_HOURS: u32 = 30;
pub const DEFAULT_FULL_MAX_AGE_DAYS: u32 = 8;
pub const DEFAULT_OPEN_SET_MAX_AGE_HOURS: u32 = 36;
pub const DEFAULT_STUCK_HEARTBEAT_MINS: u32 = 10;
pub const DEFAULT_REPEAT_FAILURES: u32 = 3;

/// Endpoints whose open documents are compared with Visma's once a day (`open_set: true` in
/// `config/endpoints`; a test keeps this list and the configs in step).
pub const OPEN_SET_ENDPOINTS: [&str; 2] = ["customer_invoices", "supplier_invoices"];

const TIERS: [Tier; 3] = [Tier::Hourly, Tier::Daily, Tier::Full];

const NEVER_RAN: &str = "never ran: no scheduler pass is recorded for this tenant (the scheduler \
                         reads the tenant list only when it starts)";

/// How old things may be before they are a problem, and how many failures in a row make one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    pub hourly_max_age: Duration,
    pub daily_max_age: Duration,
    pub full_max_age: Duration,
    pub open_set_max_age: Duration,
    pub stuck_heartbeat: Duration,
    pub repeat_failures: usize,
}

impl Thresholds {
    fn max_age(&self, tier: Tier) -> Duration {
        match tier {
            Tier::Hourly => self.hourly_max_age,
            Tier::Daily => self.daily_max_age,
            Tier::Full => self.full_max_age,
        }
    }
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            hourly_max_age: Duration::minutes(i64::from(DEFAULT_HOURLY_MAX_AGE_MINS)),
            daily_max_age: Duration::hours(i64::from(DEFAULT_DAILY_MAX_AGE_HOURS)),
            full_max_age: Duration::days(i64::from(DEFAULT_FULL_MAX_AGE_DAYS)),
            open_set_max_age: Duration::hours(i64::from(DEFAULT_OPEN_SET_MAX_AGE_HOURS)),
            stuck_heartbeat: Duration::minutes(i64::from(DEFAULT_STUCK_HEARTBEAT_MINS)),
            repeat_failures: DEFAULT_REPEAT_FAILURES as usize,
        }
    }
}

/// The `sync_run.endpoint` values of the open-set runs (`customer_invoices:open_set`, ...).
pub fn open_set_run_labels() -> Vec<String> {
    OPEN_SET_ENDPOINTS
        .iter()
        .map(|endpoint| run_label(endpoint))
        .collect()
}

/// The limits and the clock every check works from.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    limits: &'a Thresholds,
    now: DateTime<Utc>,
}

impl Ctx<'_> {
    /// How long ago `at` was; never negative (a row dated ahead of the clock is "now").
    fn age(&self, at: DateTime<Utc>) -> Duration {
        (self.now - at).max(Duration::zero())
    }
}

/// The tenant a finding is about.
struct Who<'a> {
    id: &'a str,
    name: &'a str,
}

impl Who<'_> {
    fn finding(
        &self,
        check: impl Into<String>,
        severity: Severity,
        detail: impl Into<String>,
        observed_at: Option<DateTime<Utc>>,
    ) -> Finding {
        Finding::new(
            Some(self.id),
            Some(self.name),
            check,
            severity,
            detail,
            observed_at,
        )
    }
}

/// Every check for every tenant, in tenant order, preceded by nothing and followed by the stuck
/// passes of tenants that are not in the list.
pub fn evaluate(
    tenants: &[ScheduledTenant],
    snapshot: &Snapshot,
    limits: &Thresholds,
    now: DateTime<Utc>,
) -> Vec<Finding> {
    if snapshot.passes.is_empty() {
        return vec![Finding::new(
            None,
            None,
            "scheduler_ran",
            Severity::Problem,
            "no scheduler pass is recorded in vsv2.scheduler_pass: the scheduler has not run yet",
            None,
        )];
    }
    let cx = Ctx { limits, now };
    let passes = group_passes(&snapshot.passes);
    let runs = group_runs(&snapshot.runs);
    let mut findings: Vec<Finding> = tenants
        .iter()
        .flat_map(|tenant| {
            let id = tenant.id.as_str();
            let own_passes = passes.get(id).map(Vec::as_slice).unwrap_or_default();
            let own_runs = runs.get(id).map(Vec::as_slice).unwrap_or_default();
            check_tenant(tenant, own_passes, own_runs, cx)
        })
        .collect();
    findings.extend(stuck_unlisted(tenants, &passes, cx));
    findings
}

fn check_tenant(
    tenant: &ScheduledTenant,
    passes: &[&PassRow],
    runs: &[&RunRow],
    cx: Ctx<'_>,
) -> Vec<Finding> {
    let who = Who {
        id: tenant.id.as_str(),
        name: &tenant.name,
    };
    if passes.is_empty() {
        return vec![who.finding("tenant_has_run", Severity::Problem, NEVER_RAN, None)];
    }
    let tiers: Vec<Tier> = TIERS
        .into_iter()
        .filter(|tier| tier.covers(tenant.status))
        .collect();
    let mut findings: Vec<Finding> = tiers
        .iter()
        .map(|tier| freshness(&who, passes, *tier, cx))
        .collect();
    findings.extend(tiers.iter().map(|tier| failures(&who, passes, *tier, cx)));
    findings.push(stuck(&who, passes, cx));
    // The open-set run follows the hourly sync, so it exists for the tenants the hourly tier covers.
    if Tier::Hourly.covers(tenant.status) {
        findings.extend(OPEN_SET_ENDPOINTS.iter().map(|endpoint| {
            let label = run_label(endpoint);
            let own: Vec<&RunRow> = runs
                .iter()
                .copied()
                .filter(|run| run.endpoint == label)
                .collect();
            open_set(&who, endpoint, passes, &own, cx)
        }));
    }
    findings
}

/// The tenant's passes of one tier, newest first (the order `passes` is in).
fn tier_rows<'a>(passes: &[&'a PassRow], tier: Tier) -> Vec<&'a PassRow> {
    passes
        .iter()
        .copied()
        .filter(|pass| pass.tier == tier.label())
        .collect()
}

/// When the tenant's oldest pass of any tier began (`fallback` for a tenant without passes).
fn oldest(passes: &[&PassRow], fallback: DateTime<Utc>) -> DateTime<Utc> {
    passes
        .iter()
        .map(|pass| pass.started_at)
        .min()
        .unwrap_or(fallback)
}

/// Passes by tenant id, each tenant's newest first.
fn group_passes(passes: &[PassRow]) -> HashMap<&str, Vec<&PassRow>> {
    let mut grouped: HashMap<&str, Vec<&PassRow>> = HashMap::new();
    for pass in passes {
        grouped
            .entry(pass.tenant_id.as_str())
            .or_default()
            .push(pass);
    }
    for rows in grouped.values_mut() {
        rows.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    }
    grouped
}

/// Runs by tenant id, each tenant's newest first.
fn group_runs(runs: &[RunRow]) -> HashMap<&str, Vec<&RunRow>> {
    let mut grouped: HashMap<&str, Vec<&RunRow>> = HashMap::new();
    for run in runs {
        grouped.entry(run.tenant_id.as_str()).or_default().push(run);
    }
    for rows in grouped.values_mut() {
        rows.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    }
    grouped
}

#[cfg(test)]
mod tests;
