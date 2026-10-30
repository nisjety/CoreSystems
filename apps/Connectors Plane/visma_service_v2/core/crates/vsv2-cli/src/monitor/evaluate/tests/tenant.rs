//! The picture per tenant: healthy, never ran, historical, and the defaults.

use super::*;

#[test]
fn a_healthy_active_tenant_has_nothing_but_ok_findings() {
    let (passes, runs) = healthy(ACTIVE);

    let findings = check(&[active()], passes, runs);

    assert!(not_ok(&findings).is_empty(), "{findings:#?}");
    assert_eq!(
        checks_of(&findings, ACTIVE),
        [
            "freshness_hourly",
            "freshness_daily",
            "freshness_full",
            "failures_hourly",
            "failures_daily",
            "failures_full",
            "stuck_pass",
            "open_set_customer_invoices",
            "open_set_supplier_invoices",
        ]
    );
}

#[test]
fn findings_follow_the_tenant_list_and_name_the_tenant() {
    let (mut passes, mut runs) = healthy(ACTIVE);
    let (more_passes, more_runs) = healthy(OTHER);
    passes.extend(more_passes);
    runs.extend(more_runs);

    let findings = check(&[other(), active()], passes, runs);

    let first = findings.first().expect("findings");
    let last = findings.last().expect("findings");
    assert_eq!(first.tenant_id.as_deref(), Some(OTHER));
    assert_eq!(first.tenant_name.as_deref(), Some("Aqua Pharma Group AS"));
    assert_eq!(last.tenant_id.as_deref(), Some(ACTIVE));
}

#[test]
fn observed_at_is_when_the_row_the_finding_rests_on_began() {
    let (mut passes, mut runs) = healthy(ACTIVE);
    passes.push(running(ACTIVE, "daily", ago(hours(1)), ago(mins(30))));
    passes.push(failed(ACTIVE, "hourly", ago(mins(5)), &["customers"]));
    runs[0].started_at = ago(hours(7));

    let findings = check(&[active()], passes, runs);

    assert_eq!(
        find(&findings, ACTIVE, "freshness_hourly").observed_at,
        Some(ago(mins(20))),
        "the newest succeeded pass"
    );
    assert_eq!(
        find(&findings, ACTIVE, "failures_hourly").observed_at,
        Some(ago(mins(5))),
        "the newest finished pass"
    );
    assert_eq!(
        find(&findings, ACTIVE, "stuck_pass").observed_at,
        Some(ago(mins(30))),
        "the heartbeat that stopped"
    );
    assert_eq!(
        find(&findings, ACTIVE, "open_set_customer_invoices").observed_at,
        Some(ago(hours(7))),
        "the newest open-set run"
    );
}

#[test]
fn a_tenant_with_no_pass_at_all_never_ran() {
    let (passes, runs) = healthy(ACTIVE);

    let findings = check(&[active(), other()], passes, runs);

    let never = find(&findings, OTHER, "tenant_has_run");
    assert_eq!(never.severity, Severity::Problem);
    assert!(never.detail.starts_with("never ran"), "{}", never.detail);
    assert_eq!(never.observed_at, None);
    assert_eq!(
        checks_of(&findings, OTHER),
        ["tenant_has_run"],
        "no other check for a tenant that never ran"
    );
    assert!(not_ok(&findings)
        .iter()
        .all(|(tenant, ..)| *tenant == OTHER));
}

#[test]
fn a_database_without_any_pass_gives_one_problem_and_none_per_tenant() {
    let findings = check(&[active(), other(), historical()], Vec::new(), Vec::new());

    assert_eq!(findings.len(), 1, "{findings:#?}");
    let only = &findings[0];
    assert_eq!(only.check, "scheduler_ran");
    assert_eq!(only.severity, Severity::Problem);
    assert_eq!(
        (only.tenant_id.as_deref(), only.tenant_name.as_deref()),
        (None, None)
    );
    assert!(only.detail.contains("has not run yet"), "{}", only.detail);
}

#[test]
fn a_historical_tenant_is_checked_on_the_full_tier_only() {
    let passes = vec![done(HISTORICAL, "full", ago(days(3)))];

    let findings = check(&[historical()], passes, Vec::new());

    assert_eq!(
        checks_of(&findings, HISTORICAL),
        ["freshness_full", "failures_full", "stuck_pass"],
        "no hourly, daily or open-set check"
    );
    assert!(not_ok(&findings).is_empty(), "{findings:#?}");
}

#[test]
fn a_stale_full_pass_is_a_problem_for_a_historical_tenant() {
    let passes = vec![done(HISTORICAL, "full", ago(days(9)))];

    let findings = check(&[historical()], passes, Vec::new());

    assert_eq!(
        not_ok(&findings),
        [(HISTORICAL, "freshness_full", Severity::Problem)]
    );
}

#[test]
fn old_hourly_rows_of_a_historical_tenant_are_not_judged() {
    let passes = vec![
        done(HISTORICAL, "full", ago(days(1))),
        done(HISTORICAL, "hourly", ago(days(300))),
        failed(HISTORICAL, "daily", ago(days(299)), &["customers"]),
    ];

    let findings = check(&[historical()], passes, Vec::new());

    assert!(not_ok(&findings).is_empty(), "{findings:#?}");
}

#[test]
fn a_historical_tenant_that_never_ran_is_a_problem_too() {
    let (passes, runs) = healthy(ACTIVE);

    let findings = check(&[active(), historical()], passes, runs);

    assert_eq!(
        severity(&findings, HISTORICAL, "tenant_has_run"),
        Severity::Problem
    );
}

#[test]
fn the_default_thresholds_are_the_documented_ones() {
    let defaults = Thresholds::default();

    assert_eq!(defaults.hourly_max_age, mins(180));
    assert_eq!(defaults.daily_max_age, hours(30));
    assert_eq!(defaults.full_max_age, days(8));
    assert_eq!(defaults.open_set_max_age, hours(36));
    assert_eq!(defaults.stuck_heartbeat, mins(10));
    assert_eq!(defaults.repeat_failures, 3);
}

#[test]
fn the_scheduler_and_the_monitor_agree_on_which_tiers_a_historical_tenant_runs() {
    // `check_tenant` takes its tiers from `Tier::covers`; this pins what that means today.
    let historical_tiers: Vec<&str> = TIERS
        .into_iter()
        .filter(|tier| tier.covers(TenantStatus::Historical))
        .map(Tier::label)
        .collect();
    let active_tiers: Vec<&str> = TIERS
        .into_iter()
        .filter(|tier| tier.covers(TenantStatus::Active))
        .map(Tier::label)
        .collect();

    assert_eq!(historical_tiers, ["full"]);
    assert_eq!(active_tiers, ["hourly", "daily", "full"]);
}
