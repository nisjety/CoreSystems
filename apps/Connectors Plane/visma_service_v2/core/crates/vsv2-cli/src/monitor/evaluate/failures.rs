//! Check 3, `failures_<tier>`: the newest finished pass of a tier did not fail. An endpoint that
//! failed in the last `repeat_failures` finished passes is a problem, a single failure a warning.

use vsv2_orchestrator::Tier;
use vsv2_state::monitor::PassRow;
use vsv2_state::scheduler_pass::PassStatus;

use super::{tier_rows, Ctx, Who};
use crate::monitor::report::{timestamp, Finding, Severity};

/// How many endpoint names one finding lists before it says "and N more".
const MAX_LISTED: usize = 8;

pub(super) fn failures(who: &Who<'_>, passes: &[&PassRow], tier: Tier, cx: Ctx<'_>) -> Finding {
    let check = format!("failures_{}", tier.label());
    let finished: Vec<&PassRow> = tier_rows(passes, tier)
        .into_iter()
        .filter(|pass| pass.status != PassStatus::Running)
        .collect();
    let Some(latest) = finished.first().copied() else {
        return who.finding(
            check,
            Severity::Ok,
            format!("no {} pass has finished yet", tier.label()),
            None,
        );
    };
    if latest.status == PassStatus::Succeeded {
        return who.finding(
            check,
            Severity::Ok,
            format!(
                "latest {} pass, started {}, succeeded",
                tier.label(),
                timestamp(latest.started_at)
            ),
            Some(latest.started_at),
        );
    }
    if latest.abandoned || latest.failed_names.is_empty() {
        pass_failure(who, check, tier, latest, &finished, cx)
    } else {
        endpoint_failures(who, check, tier, latest, &finished, cx)
    }
}

/// The latest pass failed on named endpoints: a problem when one of them has failed in the last
/// `repeat_failures` finished passes running, otherwise a warning.
fn endpoint_failures(
    who: &Who<'_>,
    check: String,
    tier: Tier,
    latest: &PassRow,
    finished: &[&PassRow],
    cx: Ctx<'_>,
) -> Finding {
    let repeat = cx.limits.repeat_failures;
    let mut counted: Vec<(&str, usize)> = latest
        .failed_names
        .iter()
        .map(|name| (name.as_str(), consecutive_failures(finished, name)))
        .collect();
    counted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let severity = if counted.iter().any(|(_, in_a_row)| *in_a_row >= repeat) {
        Severity::Problem
    } else {
        Severity::Warning
    };
    let listed: Vec<String> = counted
        .iter()
        .take(MAX_LISTED)
        .map(|(name, in_a_row)| format!("{name} ({})", passes_in_a_row(*in_a_row)))
        .collect();
    let more = match counted.len().saturating_sub(MAX_LISTED) {
        0 => String::new(),
        rest => format!(", and {rest} more"),
    };
    who.finding(
        check,
        severity,
        format!(
            "latest {} pass, started {}, failed: {}{more}; {repeat} in a row make an endpoint a \
             problem",
            tier.label(),
            timestamp(latest.started_at),
            listed.join(", ")
        ),
        Some(latest.started_at),
    )
}

/// How many of the newest finished passes, back to back, list `name` as failed. An abandoned pass
/// says nothing about any endpoint, so it neither extends nor breaks the run; a pass that
/// succeeded, or failed on other endpoints, ends it.
fn consecutive_failures(finished: &[&PassRow], name: &str) -> usize {
    let mut in_a_row = 0;
    for pass in finished.iter().filter(|pass| !pass.abandoned) {
        if !pass.failed_names.iter().any(|failed| failed == name) {
            break;
        }
        in_a_row += 1;
    }
    in_a_row
}

/// The latest pass failed without an endpoint to blame (the scheduler died, or the row carries no
/// names): judged on how many passes in a row failed at all.
fn pass_failure(
    who: &Who<'_>,
    check: String,
    tier: Tier,
    latest: &PassRow,
    finished: &[&PassRow],
    cx: Ctx<'_>,
) -> Finding {
    let repeat = cx.limits.repeat_failures;
    let in_a_row = finished
        .iter()
        .take_while(|pass| pass.status == PassStatus::Failed)
        .count();
    let severity = if in_a_row >= repeat {
        Severity::Problem
    } else {
        Severity::Warning
    };
    let what = if latest.abandoned {
        "was abandoned: the scheduler ended without finishing it".to_string()
    } else {
        format!(
            "failed without naming an endpoint (see the error of pass {} in vsv2.scheduler_pass)",
            latest.pass_id
        )
    };
    who.finding(
        check,
        severity,
        format!(
            "latest {} pass, started {}, {what}; {} failed, {repeat} in a row make a problem",
            tier.label(),
            timestamp(latest.started_at),
            passes_in_a_row(in_a_row)
        ),
        Some(latest.started_at),
    )
}

/// `1 pass in a row`, `3 passes in a row`.
fn passes_in_a_row(count: usize) -> String {
    format!(
        "{count} {} in a row",
        if count == 1 { "pass" } else { "passes" }
    )
}
