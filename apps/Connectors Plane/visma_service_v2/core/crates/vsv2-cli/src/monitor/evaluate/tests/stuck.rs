//! Check 2: `stuck_pass`.

use super::*;

#[test]
fn a_running_pass_whose_heartbeat_stopped_is_a_problem() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(running(ACTIVE, "hourly", ago(hours(1)), ago(mins(11))));

    let findings = check(&[active()], passes, runs);

    let stuck = find(&findings, ACTIVE, "stuck_pass");
    assert_eq!(stuck.severity, Severity::Problem);
    assert!(
        stuck.detail.contains("hourly pass begun"),
        "{}",
        stuck.detail
    );
    assert!(
        stuck.detail.contains("no heartbeat for 11 min"),
        "{}",
        stuck.detail
    );
    assert!(stuck.detail.contains("limit 10 min"), "{}", stuck.detail);
    assert_eq!(stuck.observed_at, Some(ago(mins(11))));
}

#[test]
fn a_heartbeat_exactly_at_the_limit_is_not_stuck_and_one_second_more_is() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(running(ACTIVE, "daily", ago(hours(1)), ago(mins(10))));
    let at_limit = check(&[active()], passes.clone(), runs.clone());
    passes.pop();
    passes.push(running(
        ACTIVE,
        "daily",
        ago(hours(1)),
        ago(mins(10) + Duration::seconds(1)),
    ));
    let over = check(&[active()], passes, runs);

    assert_eq!(severity(&at_limit, ACTIVE, "stuck_pass"), Severity::Ok);
    assert_eq!(severity(&over, ACTIVE, "stuck_pass"), Severity::Problem);
}

#[test]
fn a_pass_that_runs_for_days_but_still_beats_is_fine() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(running(
        ACTIVE,
        "full",
        ago(days(3)),
        ago(Duration::seconds(30)),
    ));

    let findings = check(&[active()], passes, runs);

    assert!(not_ok(&findings).is_empty(), "{findings:#?}");
}

#[test]
fn several_stuck_passes_of_one_tenant_are_one_finding_that_lists_them_all() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(running(ACTIVE, "hourly", ago(hours(2)), ago(mins(40))));
    passes.push(running(ACTIVE, "full", ago(hours(3)), ago(mins(90))));

    let findings = check(&[active()], passes, runs);

    let stuck: Vec<_> = findings
        .iter()
        .filter(|f| f.check == "stuck_pass")
        .collect();
    assert_eq!(stuck.len(), 1);
    assert!(stuck[0].detail.contains("hourly pass begun"));
    assert!(stuck[0].detail.contains("full pass begun"));
    assert_eq!(
        stuck[0].observed_at,
        Some(ago(mins(90))),
        "the oldest heartbeat"
    );
}

#[test]
fn the_stuck_limit_follows_the_threshold_that_is_passed_in() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(running(ACTIVE, "hourly", ago(hours(1)), ago(mins(20))));
    let patient = Thresholds {
        stuck_heartbeat: mins(30),
        ..Thresholds::default()
    };

    let findings = check_with(&[active()], passes, runs, &patient);

    assert_eq!(severity(&findings, ACTIVE, "stuck_pass"), Severity::Ok);
}

#[test]
fn a_stuck_pass_of_a_tenant_that_is_not_in_the_list_is_still_reported() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(running(UNLISTED, "hourly", ago(hours(1)), ago(mins(30))));

    let findings = check(&[active()], passes, runs);

    let stuck = find(&findings, UNLISTED, "stuck_pass");
    assert_eq!(stuck.severity, Severity::Problem);
    assert_eq!(stuck.tenant_name, None);
    assert!(stuck.detail.contains("not in the tenant list"));
    assert_eq!(
        checks_of(&findings, UNLISTED),
        ["stuck_pass"],
        "nothing else is said about a tenant that is not in the list"
    );
}
