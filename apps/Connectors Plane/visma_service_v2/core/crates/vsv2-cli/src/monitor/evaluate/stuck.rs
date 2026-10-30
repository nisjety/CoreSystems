//! Check 2, `stuck_pass`: no pass is `running` with a heartbeat older than the stuck limit, which
//! means the scheduler died mid-pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use vsv2_orchestrator::ScheduledTenant;
use vsv2_state::monitor::PassRow;
use vsv2_state::scheduler_pass::PassStatus;

use super::words::{human, limit_text};
use super::{Ctx, Who};
use crate::monitor::report::{timestamp, Finding, Severity};

pub(super) fn stuck(who: &Who<'_>, passes: &[&PassRow], cx: Ctx<'_>) -> Finding {
    let stuck = stuck_passes(passes, cx);
    if stuck.is_empty() {
        return who.finding(
            "stuck_pass",
            Severity::Ok,
            "no running pass has lost its heartbeat",
            None,
        );
    }
    who.finding(
        "stuck_pass",
        Severity::Problem,
        stuck_detail(&stuck, cx),
        stuck.iter().map(|pass| pass.heartbeat_at).min(),
    )
}

fn stuck_passes<'a>(passes: &[&'a PassRow], cx: Ctx<'_>) -> Vec<&'a PassRow> {
    passes
        .iter()
        .copied()
        .filter(|pass| {
            pass.status == PassStatus::Running
                && cx.age(pass.heartbeat_at) > cx.limits.stuck_heartbeat
        })
        .collect()
}

fn stuck_detail(stuck: &[&PassRow], cx: Ctx<'_>) -> String {
    let each: Vec<String> = stuck
        .iter()
        .map(|pass| {
            format!(
                "the {} pass begun {} has had no heartbeat for {}",
                pass.tier,
                timestamp(pass.started_at),
                human(cx.age(pass.heartbeat_at))
            )
        })
        .collect();
    format!(
        "{}; limit {}: the scheduler died mid-pass (a live one marks such a pass abandoned at \
         its next poll)",
        each.join("; "),
        limit_text(cx.limits.stuck_heartbeat)
    )
}

/// Stuck passes of tenants that are in the database but not in the tenant list: a dead scheduler
/// is dead for them too.
pub(super) fn stuck_unlisted(
    tenants: &[ScheduledTenant],
    passes: &HashMap<&str, Vec<&PassRow>>,
    cx: Ctx<'_>,
) -> Vec<Finding> {
    let listed: HashSet<&str> = tenants.iter().map(|tenant| tenant.id.as_str()).collect();
    let unlisted: BTreeMap<&str, &Vec<&PassRow>> = passes
        .iter()
        .filter(|(id, _)| !listed.contains(**id))
        .map(|(id, rows)| (*id, rows))
        .collect();
    unlisted
        .into_iter()
        .filter_map(|(id, rows)| {
            let stuck = stuck_passes(rows, cx);
            (!stuck.is_empty()).then(|| {
                Finding::new(
                    Some(id),
                    None,
                    "stuck_pass",
                    Severity::Problem,
                    format!(
                        "{} (this tenant is not in the tenant list)",
                        stuck_detail(&stuck, cx)
                    ),
                    stuck.iter().map(|pass| pass.heartbeat_at).min(),
                )
            })
        })
        .collect()
}
