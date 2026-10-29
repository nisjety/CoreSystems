//! Check 1: `freshness_<tier>`.

use super::*;

#[test]
fn a_hourly_pass_exactly_at_the_limit_is_fresh_and_a_second_older_is_a_problem() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes[0] = done(ACTIVE, "hourly", ago(mins(180)));
    let at_limit = check(&[active()], passes.clone(), runs.clone());
    passes[0] = done(ACTIVE, "hourly", ago(mins(180) + Duration::seconds(1)));
    let over = check(&[active()], passes, runs);

    assert_eq!(
        severity(&at_limit, ACTIVE, "freshness_hourly"),
        Severity::Ok
    );
    let finding = find(&over, ACTIVE, "freshness_hourly");
    assert_eq!(finding.severity, Severity::Problem);
    assert!(finding.detail.contains("limit 3 h"), "{}", finding.detail);
}

#[test]
fn each_tier_is_judged_against_its_own_limit() {
    let (_, runs) = healthy(ACTIVE);
    let cases = [
        ("hourly", mins(181), "freshness_hourly"),
        ("daily", hours(31), "freshness_daily"),
        ("full", days(8) + mins(1), "freshness_full"),
    ];
    for (tier, age, check_id) in cases {
        let (mut passes, _) = healthy(ACTIVE);
        passes.retain(|p| p.tier != tier);
        passes.push(done(ACTIVE, tier, ago(age)));

        let findings = check(&[active()], passes, runs.clone());

        assert_eq!(
            not_ok(&findings),
            [(ACTIVE, check_id, Severity::Problem)],
            "{tier} at {age}"
        );
    }
    // the same ages one step younger are fine
    for (tier, age) in [
        ("hourly", mins(179)),
        ("daily", hours(29)),
        ("full", days(7) + hours(23)),
    ] {
        let (mut passes, _) = healthy(ACTIVE);
        passes.retain(|p| p.tier != tier);
        passes.push(done(ACTIVE, tier, ago(age)));

        let findings = check(&[active()], passes, runs.clone());

        assert!(not_ok(&findings).is_empty(), "{tier} at {age}");
    }
}

#[test]
fn the_limits_follow_the_thresholds_that_are_passed_in() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes[0] = done(ACTIVE, "hourly", ago(mins(61)));
    let strict = Thresholds {
        hourly_max_age: mins(60),
        ..Thresholds::default()
    };

    let findings = check_with(&[active()], passes, runs, &strict);

    assert_eq!(
        not_ok(&findings),
        [(ACTIVE, "freshness_hourly", Severity::Problem)]
    );
}

#[test]
fn a_stale_tier_says_how_its_latest_pass_ended() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes[0] = done(ACTIVE, "hourly", ago(hours(5) + mins(12)));
    passes.push(failed(ACTIVE, "hourly", ago(hours(2)), &["customers"]));

    let findings = check(&[active()], passes, runs);

    let detail = &find(&findings, ACTIVE, "freshness_hourly").detail;
    assert!(detail.contains("started 5 h 12 min ago"), "{detail}");
    assert!(detail.contains("latest hourly pass: failed"), "{detail}");
}

#[test]
fn a_success_newer_than_a_failure_makes_the_tier_fresh_again() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes[0] = done(ACTIVE, "hourly", ago(mins(10)));
    passes.push(failed(ACTIVE, "hourly", ago(hours(5)), &["customers"]));

    let findings = check(&[active()], passes, runs);

    assert!(not_ok(&findings).is_empty(), "{findings:#?}");
}

#[test]
fn a_tier_that_has_not_succeeded_yet_is_a_warning_inside_its_limit_and_a_problem_past_it() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.retain(|p| p.tier != "hourly");
    let mut inside = passes.clone();
    inside.push(failed(ACTIVE, "hourly", ago(hours(2)), &["customers"]));
    let mut past = passes;
    past.push(failed(ACTIVE, "hourly", ago(hours(4)), &["customers"]));

    let early = check(&[active()], inside, runs.clone());
    let late = check(&[active()], past, runs);

    let warned = find(&early, ACTIVE, "freshness_hourly");
    assert_eq!(warned.severity, Severity::Warning);
    assert!(
        warned.detail.contains("has succeeded yet"),
        "{}",
        warned.detail
    );
    assert!(warned
        .detail
        .contains("the first hourly pass began 2 h ago"));
    assert_eq!(
        severity(&late, ACTIVE, "freshness_hourly"),
        Severity::Problem
    );
}

#[test]
fn the_grace_for_a_first_pass_counts_from_the_first_pass_not_the_latest() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.retain(|p| p.tier != "hourly");
    passes.push(failed(ACTIVE, "hourly", ago(hours(10)), &["customers"]));
    passes.push(failed(ACTIVE, "hourly", ago(mins(5)), &["customers"]));

    let findings = check(&[active()], passes, runs);

    assert_eq!(
        severity(&findings, ACTIVE, "freshness_hourly"),
        Severity::Problem,
        "ten hours of failing passes, though the latest began five minutes ago"
    );
}

#[test]
fn a_tier_with_no_pass_at_all_is_judged_from_the_tenants_first_pass() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes.retain(|p| p.tier != "daily");
    passes.push(done(ACTIVE, "hourly", ago(days(5))));
    let old = check(&[active()], passes.clone(), runs.clone());
    passes.retain(|p| p.started_at > ago(hours(4)));
    passes.push(done(ACTIVE, "full", ago(hours(2))));
    let young = check(&[active()], passes, runs);

    let problem = find(&old, ACTIVE, "freshness_daily");
    assert_eq!(problem.severity, Severity::Problem);
    assert!(problem.detail.contains("first pass (of any tier)"));
    assert!(problem.detail.contains("no daily pass is recorded"));
    assert_eq!(
        severity(&young, ACTIVE, "freshness_daily"),
        Severity::Warning
    );
}

#[test]
fn a_pass_dated_ahead_of_the_clock_counts_as_just_now() {
    let (mut passes, runs) = healthy(ACTIVE);
    passes[0] = done(ACTIVE, "hourly", now() + mins(3));

    let findings = check(&[active()], passes, runs);

    let finding = find(&findings, ACTIVE, "freshness_hourly");
    assert_eq!(finding.severity, Severity::Ok);
    assert!(finding.detail.contains("under 1 min"), "{}", finding.detail);
}
