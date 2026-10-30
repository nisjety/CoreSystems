//! Check 4, `open_set_<endpoint>`: the newest open-set run of an active tenant is young enough and
//! did not fail.

use chrono::Duration;
use vsv2_orchestrator::Tier;
use vsv2_state::monitor::{PassRow, RunRow};
use vsv2_state::run_history::RunStatus;

use super::words::{human, limit_text};
use super::{tier_rows, Ctx, Who};
use crate::monitor::report::{timestamp, Finding, Severity};

/// The open-set run follows a successful hourly sync within a day, so a tenant whose first hourly
/// pass is younger than this has not missed one yet.
const OPEN_SET_FIRST_RUN_GRACE_HOURS: i64 = 24;

pub(super) fn open_set(
    who: &Who<'_>,
    endpoint: &str,
    passes: &[&PassRow],
    runs: &[&RunRow],
    cx: Ctx<'_>,
) -> Finding {
    let check = format!("open_set_{endpoint}");
    let Some(latest) = runs.first().copied() else {
        return open_set_missing(who, check, passes, cx);
    };
    let limit = cx.limits.open_set_max_age;
    let age = cx.age(latest.started_at);
    let when = format!("{} ago ({})", human(age), timestamp(latest.started_at));
    let observed = Some(latest.started_at);
    if age > limit {
        return who.finding(
            check,
            Severity::Problem,
            format!(
                "latest open-set run ({}) started {when}, over the limit of {}",
                run_status_text(latest.status),
                limit_text(limit)
            ),
            observed,
        );
    }
    match latest.status {
        RunStatus::Failed => open_set_failed(who, check, latest, runs, &when, cx),
        RunStatus::PartialSuccess => who.finding(
            check,
            Severity::Warning,
            format!(
                "latest open-set run ended partially {when}: documents were left for the next \
                 run (run {} in vsv2.sync_run)",
                latest.run_id
            ),
            observed,
        ),
        _ => who.finding(
            check,
            Severity::Ok,
            format!(
                "latest open-set run {} {when}, limit {}",
                run_status_text(latest.status),
                limit_text(limit)
            ),
            observed,
        ),
    }
}

/// The latest run failed: a warning, and a problem once `repeat_failures` runs in a row failed.
fn open_set_failed(
    who: &Who<'_>,
    check: String,
    latest: &RunRow,
    runs: &[&RunRow],
    when: &str,
    cx: Ctx<'_>,
) -> Finding {
    let repeat = cx.limits.repeat_failures;
    let in_a_row = runs
        .iter()
        .take_while(|run| run.status == RunStatus::Failed)
        .count();
    let severity = if in_a_row >= repeat {
        Severity::Problem
    } else {
        Severity::Warning
    };
    who.finding(
        check,
        severity,
        format!(
            "latest open-set run failed {when}; {} failed, {repeat} in a row make a problem \
             (run {} in vsv2.sync_run)",
            runs_in_a_row(in_a_row),
            latest.run_id
        ),
        Some(latest.started_at),
    )
}

/// No open-set run is recorded. A warning until the tenant's first hourly pass is a day old (the
/// run follows a successful hourly sync of the endpoint), a problem after.
fn open_set_missing(who: &Who<'_>, check: String, passes: &[&PassRow], cx: Ctx<'_>) -> Finding {
    let Some(first) = tier_rows(passes, Tier::Hourly)
        .last()
        .map(|pass| pass.started_at)
    else {
        return who.finding(
            check,
            Severity::Warning,
            "no open-set run is recorded, and no hourly pass has run yet",
            None,
        );
    };
    let since = cx.age(first);
    let (severity, tail) = if since > Duration::hours(OPEN_SET_FIRST_RUN_GRACE_HOURS) {
        (
            Severity::Problem,
            "it is due once a day after a successful hourly sync, and needs BigQuery staging",
        )
    } else {
        (Severity::Warning, "the first run follows within a day")
    };
    who.finding(
        check,
        severity,
        format!(
            "no open-set run is recorded; the first hourly pass began {} ago ({}); {tail}",
            human(since),
            timestamp(first)
        ),
        Some(first),
    )
}

fn run_status_text(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Pending => "pending",
        RunStatus::Running => "running",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::PartialSuccess => "partial",
    }
}

/// `1 run in a row`, `3 runs in a row`.
fn runs_in_a_row(count: usize) -> String {
    format!(
        "{count} {} in a row",
        if count == 1 { "run" } else { "runs" }
    )
}
