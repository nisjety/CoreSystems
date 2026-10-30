//! The checks, one behaviour per test, on fixed timestamps: nothing here reads a clock or a
//! database.

use std::path::PathBuf;

use uuid::Uuid;
use vsv2_common::TenantStatus;
use vsv2_orchestrator::EndpointRegistry;

use super::*;
use crate::monitor::report::{Finding, Severity};
use vsv2_orchestrator::Tier;
use vsv2_state::monitor::{PassRow, RunRow, Snapshot};
use vsv2_state::run_history::RunStatus;
use vsv2_state::scheduler_pass::PassStatus;

const ACTIVE: &str = "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd";
const OTHER: &str = "ffa4a061-7717-11ee-970e-025417856183";
const HISTORICAL: &str = "9b1fa69b-5568-11ee-960c-025417856183";
const UNLISTED: &str = "00000000-0000-4000-8000-000000000001";

fn at(text: &str) -> DateTime<Utc> {
    text.parse().expect("RFC 3339 timestamp")
}

/// The fixed "now" of every test.
fn now() -> DateTime<Utc> {
    at("2026-10-13T12:00:00Z")
}

fn mins(count: i64) -> Duration {
    Duration::minutes(count)
}

fn hours(count: i64) -> Duration {
    Duration::hours(count)
}

fn days(count: i64) -> Duration {
    Duration::days(count)
}

fn ago(span: Duration) -> DateTime<Utc> {
    now() - span
}

fn tenant(id: &str, name: &str, status: TenantStatus) -> ScheduledTenant {
    ScheduledTenant {
        id: vsv2_common::TenantId::new(id),
        name: name.to_string(),
        status,
    }
}

fn active() -> ScheduledTenant {
    tenant(ACTIVE, "Aqua Pharma AS", TenantStatus::Active)
}

fn other() -> ScheduledTenant {
    tenant(OTHER, "Aqua Pharma Group AS", TenantStatus::Active)
}

fn historical() -> ScheduledTenant {
    tenant(HISTORICAL, "Consult AS", TenantStatus::Historical)
}

fn pass(tenant: &str, tier: &str, status: PassStatus, started: DateTime<Utc>) -> PassRow {
    PassRow {
        pass_id: Uuid::new_v4(),
        tenant_id: tenant.to_string(),
        tier: tier.to_string(),
        status,
        started_at: started,
        finished_at: (status != PassStatus::Running).then_some(started + mins(2)),
        heartbeat_at: started,
        failed_names: Vec::new(),
        abandoned: false,
    }
}

fn done(tenant: &str, tier: &str, started: DateTime<Utc>) -> PassRow {
    pass(tenant, tier, PassStatus::Succeeded, started)
}

fn failed(tenant: &str, tier: &str, started: DateTime<Utc>, names: &[&str]) -> PassRow {
    PassRow {
        failed_names: names.iter().map(|name| name.to_string()).collect(),
        ..pass(tenant, tier, PassStatus::Failed, started)
    }
}

fn abandoned(tenant: &str, tier: &str, started: DateTime<Utc>) -> PassRow {
    PassRow {
        abandoned: true,
        ..pass(tenant, tier, PassStatus::Failed, started)
    }
}

fn running(tenant: &str, tier: &str, started: DateTime<Utc>, heartbeat: DateTime<Utc>) -> PassRow {
    PassRow {
        heartbeat_at: heartbeat,
        ..pass(tenant, tier, PassStatus::Running, started)
    }
}

fn open_run(tenant: &str, endpoint: &str, status: RunStatus, started: DateTime<Utc>) -> RunRow {
    RunRow {
        run_id: Uuid::new_v4(),
        tenant_id: tenant.to_string(),
        endpoint: run_label(endpoint),
        status,
        started_at: started,
        finished_at: Some(started + Duration::seconds(2)),
    }
}

/// A tenant that is up to date on every tier, with a recent open-set run for both endpoints.
fn healthy(tenant: &str) -> (Vec<PassRow>, Vec<RunRow>) {
    let passes = vec![
        done(tenant, "hourly", ago(mins(20))),
        done(tenant, "daily", ago(hours(3))),
        done(tenant, "full", ago(days(2))),
    ];
    let runs = OPEN_SET_ENDPOINTS
        .iter()
        .map(|endpoint| open_run(tenant, endpoint, RunStatus::Succeeded, ago(hours(5))))
        .collect();
    (passes, runs)
}

fn snapshot(passes: Vec<PassRow>, runs: Vec<RunRow>) -> Snapshot {
    Snapshot {
        database_time: now(),
        passes,
        runs,
    }
}

fn check(tenants: &[ScheduledTenant], passes: Vec<PassRow>, runs: Vec<RunRow>) -> Vec<Finding> {
    evaluate(
        tenants,
        &snapshot(passes, runs),
        &Thresholds::default(),
        now(),
    )
}

fn check_with(
    tenants: &[ScheduledTenant],
    passes: Vec<PassRow>,
    runs: Vec<RunRow>,
    limits: &Thresholds,
) -> Vec<Finding> {
    evaluate(tenants, &snapshot(passes, runs), limits, now())
}

fn find<'a>(findings: &'a [Finding], tenant_id: &str, check: &str) -> &'a Finding {
    findings
        .iter()
        .find(|f| f.tenant_id.as_deref() == Some(tenant_id) && f.check == check)
        .unwrap_or_else(|| panic!("no {check} finding for {tenant_id} in {findings:#?}"))
}

fn severity(findings: &[Finding], tenant_id: &str, check: &str) -> Severity {
    find(findings, tenant_id, check).severity
}

fn checks_of<'a>(findings: &'a [Finding], tenant_id: &str) -> Vec<&'a str> {
    findings
        .iter()
        .filter(|f| f.tenant_id.as_deref() == Some(tenant_id))
        .map(|f| f.check.as_str())
        .collect()
}

fn not_ok(findings: &[Finding]) -> Vec<(&str, &str, Severity)> {
    findings
        .iter()
        .filter(|f| f.severity != Severity::Ok)
        .map(|f| {
            (
                f.tenant_id.as_deref().unwrap_or("-"),
                f.check.as_str(),
                f.severity,
            )
        })
        .collect()
}

mod failures;
mod freshness;
mod open_set;
mod stuck;
mod tenant;
