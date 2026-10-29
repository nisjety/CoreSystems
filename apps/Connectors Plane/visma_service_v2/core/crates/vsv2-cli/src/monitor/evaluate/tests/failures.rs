//! Check 3: `failures_<tier>`.

use super::*;

#[test]
fn a_single_failure_is_a_warning_that_names_the_endpoint() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(failed(ACTIVE, "hourly", ago(mins(5)), &["customers"]));

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "failures_hourly");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(
        finding.detail.contains("customers (1 pass in a row)"),
        "{}",
        finding.detail
    );
}

#[test]
fn an_endpoint_that_failed_in_the_last_three_passes_is_a_problem() {
    let (_, runs) = healthy(ACTIVE);
    let passes = vec![
        done(ACTIVE, "hourly", ago(hours(2))),
        failed(ACTIVE, "hourly", ago(mins(50)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(35)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(20)), &["customers"]),
        done(ACTIVE, "daily", ago(hours(3))),
        done(ACTIVE, "full", ago(days(2))),
    ];

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "failures_hourly");
    assert_eq!(finding.severity, Severity::Problem);
    assert!(
        finding.detail.contains("customers (3 passes in a row)"),
        "{}",
        finding.detail
    );
    assert!(finding
        .detail
        .contains("3 in a row make an endpoint a problem"));
}

#[test]
fn two_failures_in_a_row_are_still_only_a_warning() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(failed(ACTIVE, "hourly", ago(mins(10)), &["customers"]));
    passes.push(failed(ACTIVE, "hourly", ago(mins(5)), &["customers"]));

    let findings = check(&[active()], passes, runs);

    assert_eq!(
        severity(&findings, ACTIVE, "failures_hourly"),
        Severity::Warning
    );
}

#[test]
fn a_pass_that_succeeded_ends_the_run_of_failures() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(failed(ACTIVE, "hourly", ago(hours(4)), &["customers"]));
    passes.push(failed(ACTIVE, "hourly", ago(hours(3)), &["customers"]));
    passes.push(done(ACTIVE, "hourly", ago(hours(2))));
    passes.push(failed(ACTIVE, "hourly", ago(mins(5)), &["customers"]));

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "failures_hourly");
    assert_eq!(finding.severity, Severity::Warning);
    assert!(
        finding.detail.contains("customers (1 pass in a row)"),
        "{}",
        finding.detail
    );
}

#[test]
fn a_pass_that_failed_other_endpoints_ends_the_run_of_failures() {
    let (_, runs) = healthy(ACTIVE);
    let passes = vec![
        failed(ACTIVE, "hourly", ago(mins(60)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(45)), &["shipments"]),
        failed(ACTIVE, "hourly", ago(mins(30)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(15)), &["customers"]),
        done(ACTIVE, "daily", ago(hours(3))),
        done(ACTIVE, "full", ago(days(2))),
    ];

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "failures_hourly");
    assert_eq!(finding.severity, Severity::Warning, "customers: 2 in a row");
    assert!(
        finding.detail.contains("customers (2 passes in a row)"),
        "{}",
        finding.detail
    );
}

#[test]
fn a_pass_with_one_repeated_and_one_new_failure_is_a_problem_that_lists_both_worst_first() {
    let (_, runs) = healthy(ACTIVE);
    let passes = vec![
        failed(ACTIVE, "hourly", ago(mins(50)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(35)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(20)), &["shipments", "customers"]),
        done(ACTIVE, "daily", ago(hours(3))),
        done(ACTIVE, "full", ago(days(2))),
    ];

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "failures_hourly");
    assert_eq!(finding.severity, Severity::Problem);
    assert!(
        finding
            .detail
            .contains("customers (3 passes in a row), shipments (1 pass in a row)"),
        "{}",
        finding.detail
    );
}

#[test]
fn a_pass_that_is_still_running_neither_counts_nor_breaks_the_run() {
    let (_, runs) = healthy(ACTIVE);
    let passes = vec![
        failed(ACTIVE, "hourly", ago(mins(50)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(35)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(20)), &["customers"]),
        running(ACTIVE, "hourly", ago(mins(5)), ago(Duration::seconds(20))),
        done(ACTIVE, "daily", ago(hours(3))),
        done(ACTIVE, "full", ago(days(2))),
    ];

    let findings = check(&[active()], passes, runs);

    assert_eq!(
        severity(&findings, ACTIVE, "failures_hourly"),
        Severity::Problem
    );
    assert_eq!(severity(&findings, ACTIVE, "stuck_pass"), Severity::Ok);
}

#[test]
fn an_abandoned_pass_neither_extends_nor_breaks_the_run_of_failures() {
    let (_, runs) = healthy(ACTIVE);
    let passes = vec![
        failed(ACTIVE, "hourly", ago(mins(80)), &["customers"]),
        abandoned(ACTIVE, "hourly", ago(mins(65))),
        failed(ACTIVE, "hourly", ago(mins(45)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(20)), &["customers"]),
        done(ACTIVE, "daily", ago(hours(3))),
        done(ACTIVE, "full", ago(days(2))),
    ];

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "failures_hourly");
    assert_eq!(finding.severity, Severity::Problem);
    assert!(
        finding.detail.contains("customers (3 passes in a row)"),
        "{}",
        finding.detail
    );
}

#[test]
fn an_abandoned_latest_pass_is_a_warning_and_three_in_a_row_a_problem() {
    let (_, runs) = healthy(ACTIVE);
    let once = vec![
        done(ACTIVE, "hourly", ago(hours(2))),
        abandoned(ACTIVE, "hourly", ago(mins(30))),
        done(ACTIVE, "daily", ago(hours(3))),
        done(ACTIVE, "full", ago(days(2))),
    ];
    let thrice = vec![
        done(ACTIVE, "hourly", ago(hours(2))),
        abandoned(ACTIVE, "hourly", ago(mins(75))),
        abandoned(ACTIVE, "hourly", ago(mins(50))),
        failed(ACTIVE, "hourly", ago(mins(30)), &[]),
        done(ACTIVE, "daily", ago(hours(3))),
        done(ACTIVE, "full", ago(days(2))),
    ];

    let one = check(&[active()], once, runs.clone());
    let three = check(&[active()], thrice, runs);

    let warned = find(&one, ACTIVE, "failures_hourly");
    assert_eq!(warned.severity, Severity::Warning);
    assert!(warned.detail.contains("was abandoned"), "{}", warned.detail);
    assert!(
        warned.detail.contains("1 pass in a row failed"),
        "{}",
        warned.detail
    );
    let problem = find(&three, ACTIVE, "failures_hourly");
    assert_eq!(problem.severity, Severity::Problem);
    assert!(problem.detail.contains("failed without naming an endpoint"));
    assert!(
        problem.detail.contains("3 passes in a row failed"),
        "{}",
        problem.detail
    );
}

#[test]
fn one_failure_in_a_row_is_a_problem_when_the_threshold_is_one() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.push(failed(ACTIVE, "hourly", ago(mins(5)), &["customers"]));
    let touchy = Thresholds {
        repeat_failures: 1,
        ..Thresholds::default()
    };

    let findings = check_with(&[active()], passes, runs, &touchy);

    assert_eq!(
        severity(&findings, ACTIVE, "failures_hourly"),
        Severity::Problem
    );
}

#[test]
fn a_long_list_of_failed_endpoints_is_cut_after_eight() {
    let (mut passes, runs) = healthy(ACTIVE);
    let names: Vec<String> = (1..=12).map(|n| format!("endpoint_{n:02}")).collect();
    let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
    passes.push(failed(ACTIVE, "hourly", ago(mins(5)), &borrowed));

    let findings = check(&[active()], passes, runs);

    let detail = &find(&findings, ACTIVE, "failures_hourly").detail;
    assert!(detail.contains("endpoint_08"), "{detail}");
    assert!(!detail.contains("endpoint_09"), "{detail}");
    assert!(detail.contains("and 4 more"), "{detail}");
}

#[test]
fn failures_are_judged_per_tier() {
    let (_, runs) = healthy(ACTIVE);
    let passes = vec![
        failed(ACTIVE, "hourly", ago(mins(50)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(35)), &["customers"]),
        failed(ACTIVE, "hourly", ago(mins(20)), &["customers"]),
        done(ACTIVE, "daily", ago(hours(3))),
        done(ACTIVE, "full", ago(days(2))),
    ];

    let findings = check(&[active()], passes, runs);

    assert_eq!(
        not_ok(&findings),
        [
            (ACTIVE, "freshness_hourly", Severity::Warning),
            (ACTIVE, "failures_hourly", Severity::Problem)
        ],
        "the hourly tier has never succeeded (its first pass is younger than the limit); the other tiers are untouched"
    );
}

#[test]
fn a_tier_with_no_finished_pass_has_nothing_to_report_on_failures() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.retain(|p| p.tier != "hourly");
    passes.push(running(
        ACTIVE,
        "hourly",
        ago(mins(3)),
        ago(Duration::seconds(10)),
    ));

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "failures_hourly");
    assert_eq!(finding.severity, Severity::Ok);
    assert!(finding.detail.contains("no hourly pass has finished yet"));
}
