//! Check 1, `freshness_<tier>`: the newest succeeded pass of a tier is younger than the tier's
//! limit.

use vsv2_orchestrator::Tier;
use vsv2_state::monitor::PassRow;
use vsv2_state::scheduler_pass::PassStatus;

use super::words::{human, limit_text};
use super::{oldest, tier_rows, Ctx, Who};
use crate::monitor::report::{timestamp, Finding, Severity};

pub(super) fn freshness(who: &Who<'_>, passes: &[&PassRow], tier: Tier, cx: Ctx<'_>) -> Finding {
    let check = format!("freshness_{}", tier.label());
    let limit = cx.limits.max_age(tier);
    let rows = tier_rows(passes, tier);
    let Some(done) = rows
        .iter()
        .find(|pass| pass.status == PassStatus::Succeeded)
    else {
        return never_succeeded(who, check, passes, &rows, tier, cx);
    };
    let age = cx.age(done.started_at);
    let text = format!(
        "last succeeded {} pass started {} ago ({}), limit {}",
        tier.label(),
        human(age),
        timestamp(done.started_at),
        limit_text(limit)
    );
    if age > limit {
        let latest = latest_note(&rows, tier, cx);
        who.finding(
            check,
            Severity::Problem,
            format!("{text}{latest}"),
            Some(done.started_at),
        )
    } else {
        who.finding(check, Severity::Ok, text, Some(done.started_at))
    }
}

/// No pass of the tier has succeeded. Within one limit of the first pass that is a tier still on
/// its first pass (a warning); beyond it, a tier that never gets through (a problem).
fn never_succeeded(
    who: &Who<'_>,
    check: String,
    passes: &[&PassRow],
    rows: &[&PassRow],
    tier: Tier,
    cx: Ctx<'_>,
) -> Finding {
    let limit = cx.limits.max_age(tier);
    let (first, which) = match rows.last() {
        Some(pass) => (pass.started_at, format!("the first {} pass", tier.label())),
        None => (
            oldest(passes, cx.now),
            "this tenant's first pass (of any tier)".to_string(),
        ),
    };
    let since = cx.age(first);
    let severity = if since > limit {
        Severity::Problem
    } else {
        Severity::Warning
    };
    who.finding(
        check,
        severity,
        format!(
            "no {} pass has succeeded yet: {which} began {} ago ({}), limit {}{}",
            tier.label(),
            human(since),
            timestamp(first),
            limit_text(limit),
            latest_note(rows, tier, cx)
        ),
        Some(first),
    )
}

/// `; latest hourly pass: failed, started 2 h ago`, or what is known when there is none.
fn latest_note(rows: &[&PassRow], tier: Tier, cx: Ctx<'_>) -> String {
    match rows.first() {
        Some(pass) => format!("; latest {} pass: {}", tier.label(), describe(pass, cx)),
        None => format!("; no {} pass is recorded", tier.label()),
    }
}

fn describe(pass: &PassRow, cx: Ctx<'_>) -> String {
    let started = format!("started {} ago", human(cx.age(pass.started_at)));
    match pass.status {
        PassStatus::Running => format!(
            "running, {started}, heartbeat {} ago",
            human(cx.age(pass.heartbeat_at))
        ),
        PassStatus::Succeeded => format!("succeeded, {started}"),
        PassStatus::Failed if pass.abandoned => format!("abandoned, {started}"),
        PassStatus::Failed => format!("failed, {started}"),
    }
}
